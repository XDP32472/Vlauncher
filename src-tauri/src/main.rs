use std::{fs,path::PathBuf,sync::Mutex,time::Duration};
use serde::{Deserialize,Serialize};
use tauri::Manager;
use sha2::{Digest,Sha512};
use launcher_core::paths::{AppPaths,InstanceId,safe_join};
const API:&str="https://api.modrinth.com/v2";
const MAX_MOD_BYTES:u64=536870912;
struct State{paths:AppPaths,active:Mutex<Option<String>>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Profile{id:String,name:String,minecraft_version:String,loader:String}
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct ProfileResult{profiles:Vec<Profile>,active_profile_id:Option<String>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Installed{project_id:String,file_name:String,sha512:String}
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Store{profiles:Vec<Profile>,active_profile_id:Option<String>}
#[derive(Serialize,Deserialize)]
struct Hit{project_id:String,title:String,description:String,author:String,icon_url:Option<String>,#[serde(default)]categories:Vec<String>,downloads:u64}
#[derive(Deserialize)]
struct Version{files:Vec<RemoteFile>,#[serde(default)]dependencies:Vec<Dependency>}
#[derive(Deserialize)]
struct Dependency{dependency_type:String}
#[derive(Deserialize)]
struct RemoteFile{url:String,filename:String,size:u64,hashes:Hashes,#[serde(default)]primary:bool}
#[derive(Deserialize)]
struct Hashes{sha512:String}

#[derive(Deserialize)]
struct GameVersionEntry { id:String, url:String, sha1:String }
#[derive(Deserialize)]
struct GameManifest { versions:Vec<GameVersionEntry> }
#[derive(Deserialize)]
struct GameVersionInfo { id:String, downloads:std::collections::HashMap<String,GameDownload> }
#[derive(Deserialize)]
struct GameDownload { url:String, sha1:String, size:u64 }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct GameClientResult { version_id:String, size:u64, client_jar:String }
fn valid_mojang_url(raw:&str)->Result<reqwest::Url,String>{
 let u=reqwest::Url::parse(raw).map_err(|e|e.to_string())?;
 let host=u.host_str().unwrap_or("");
 if u.scheme()!="https"||!(host=="mojang.com"||host.ends_with(".mojang.com")){return Err("Nieoczekiwany adres pliku Minecrafta".into())}
 Ok(u)
}
async fn verified_mojang_download(c:&reqwest::Client,url:&str,sha1:&str,size:u64)->Result<Vec<u8>,String>{
 if size>536870912{return Err("Plik Minecrafta przekracza limit 512 MiB".into())}
 let bytes=c.get(valid_mojang_url(url)?).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.bytes().await.map_err(|e|e.to_string())?;
 if bytes.len() as u64!=size{return Err("Rozmiar pliku Minecrafta nie zgadza się z manifestem".into())}
 let digest=hex::encode(sha1::Sha1::digest(&bytes));
 if !digest.eq_ignore_ascii_case(sha1){return Err("Weryfikacja SHA-1 pliku Minecrafta nie powiodła się".into())}
 Ok(bytes.to_vec())
}
fn id(p:&Profile)->Result<InstanceId,String>{InstanceId::parse(p.id.clone()).map_err(|e|e.to_string())}
fn store_path(s:&State)->PathBuf{s.paths.root().join("profiles.json")}
fn read_store(s:&State)->Result<Store,String>{match fs::read(store_path(s)){Ok(b)=>serde_json::from_slice(&b).map_err(|e|e.to_string()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(Store{profiles:vec![],active_profile_id:None}),Err(e)=>Err(e.to_string())}}
fn write_store(s:&State,x:&Store)->Result<(),String>{launcher_core::store::write_json_atomic(&store_path(s),x).map_err(|e|e.to_string())}
fn profile(s:&State,key:&str)->Result<Profile,String>{read_store(s)?.profiles.into_iter().find(|p|p.id==key).ok_or("Nie znaleziono profilu".into())}
fn mods_dir(s:&State,p:&Profile)->Result<PathBuf,String>{let d=s.paths.instance_dir(&id(p)?).join("game").join("mods");fs::create_dir_all(&d).map_err(|e|e.to_string())?;Ok(d)}
fn installed_path(s:&State,p:&Profile)->Result<PathBuf,String>{Ok(s.paths.instance_dir(&id(p)?).join("installed-mods.json"))}
fn installed(s:&State,p:&Profile)->Result<Vec<Installed>,String>{match fs::read(installed_path(s,p)?){Ok(b)=>serde_json::from_slice(&b).map_err(|e|e.to_string()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(vec![]),Err(e)=>Err(e.to_string())}}
fn http()->Result<reqwest::Client,String>{reqwest::Client::builder().user_agent("Vlauncher/0.1 (github.com/XDP32472/Vlauncher)").timeout(Duration::from_secs(60)).build().map_err(|e|e.to_string())}

#[tauri::command]
async fn install_game_client(state:tauri::State<'_,State>,profile_id:String)->Result<GameClientResult,String>{
 let p=profile(&state,&profile_id)?;
 let c=http()?;
 let manifest=c.get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<GameManifest>().await.map_err(|e|e.to_string())?;
 let entry=manifest.versions.into_iter().find(|v|v.id==p.minecraft_version).ok_or("Nie znaleziono tej wersji w manifeście Mojang")?;
 let meta_bytes=c.get(valid_mojang_url(&entry.url)?).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.bytes().await.map_err(|e|e.to_string())?;
 if !hex::encode(sha1::Sha1::digest(&meta_bytes)).eq_ignore_ascii_case(&entry.sha1){return Err("Weryfikacja metadanych Minecrafta nie powiodła się".into())}
 let info:GameVersionInfo=serde_json::from_slice(&meta_bytes).map_err(|e|e.to_string())?;
 if info.id!=p.minecraft_version{return Err("Identyfikator wersji w metadanych jest niezgodny".into())}
 let client=info.downloads.get("client").ok_or("Ta wersja nie udostępnia pliku klienta")?;
 let jar=verified_mojang_download(&c,&client.url,&client.sha1,client.size).await?;
 let game=mods_dir(&state,&p)?.parent().ok_or("Nie można ustalić katalogu gry")?.to_path_buf();
 let json_rel=format!("versions/{}/{}.json",p.minecraft_version,p.minecraft_version);
 let jar_rel=format!("versions/{}/{}.jar",p.minecraft_version,p.minecraft_version);
 let json_path=safe_join(&game,&json_rel).map_err(|e|e.to_string())?;
 let jar_path=safe_join(&game,&jar_rel).map_err(|e|e.to_string())?;
 let parent=json_path.parent().ok_or("Nie można ustalić katalogu wersji")?;
 fs::create_dir_all(parent).map_err(|e|e.to_string())?;
 fs::write(&json_path,&meta_bytes).map_err(|e|e.to_string())?;
 fs::write(&jar_path,&jar).map_err(|e|e.to_string())?;
 Ok(GameClientResult{version_id:p.minecraft_version,size:jar.len() as u64,client_jar:jar_path.to_string_lossy().into_owned()})
}

#[tauri::command]fn app_status()->String{"Vlauncher 0.1 · profile and mod management".into()}
#[tauri::command]fn list_profiles(state:tauri::State<'_,State>)->Result<ProfileResult,String>{let s=read_store(&state)?;let active=state.active.lock().map_err(|e|e.to_string())?.clone().or(s.active_profile_id).or_else(||s.profiles.first().map(|p|p.id.clone()));Ok(ProfileResult{profiles:s.profiles,active_profile_id:active})}
#[tauri::command]fn create_profile(state:tauri::State<'_,State>,name:String,minecraft_version:String,loader:String)->Result<Profile,String>{let name=name.trim();let v=minecraft_version.trim();let l=loader.to_lowercase();if name.is_empty()||name.len()>48{return Err("Nazwa profilu musi mieć od 1 do 48 znaków".into())}if v.is_empty()||v.len()>32||!v.chars().all(|c|c.is_ascii_alphanumeric()||c=='.'||c=='-'){return Err("Nieprawidłowa wersja Minecrafta".into())}if l!="fabric"&&l!="vanilla"{return Err("Obsługiwane profile: Fabric lub Vanilla".into())}let p=Profile{id:uuid::Uuid::new_v4().simple().to_string(),name:name.into(),minecraft_version:v.into(),loader:l};let mut s=read_store(&state)?;s.profiles.push(p.clone());s.active_profile_id=Some(p.id.clone());write_store(&state,&s)?;*state.active.lock().map_err(|e|e.to_string())?=Some(p.id.clone());fs::create_dir_all(mods_dir(&state,&p)?).map_err(|e|e.to_string())?;Ok(p)}
#[tauri::command]async fn search_modrinth(query:String,game_version:String,loader:String)->Result<Vec<Hit>,String>{let facets=serde_json::json!([["project_type:mod"],[format!("versions:{game_version}")],[format!("categories:{loader}")]]).to_string();let res=http()?.get(format!("{API}/search")).query(&[("query",query.as_str()),("facets",facets.as_str()),("limit","40")]).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<serde_json::Value>().await.map_err(|e|e.to_string())?;serde_json::from_value(res["hits"].clone()).map_err(|e|e.to_string())}
#[tauri::command]fn list_installed_mods(state:tauri::State<'_,State>,profile_id:String)->Result<Vec<Installed>,String>{let p=profile(&state,&profile_id)?;let _=mods_dir(&state,&p)?;installed(&state,&p)}
#[tauri::command]async fn install_mod(state:tauri::State<'_,State>,profile_id:String,project_id:String,minecraft_version:String,loader:String)->Result<Installed,String>{let p=profile(&state,&profile_id)?;if p.loader!="fabric"||loader!="fabric"{return Err("Mody są obecnie dostępne tylko dla profili Fabric".into())}if project_id.len()>64||!project_id.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'||c=='-'){return Err("Nieprawidłowy identyfikator moda".into())}let c=http()?;let versions=c.get(format!("{API}/project/{project_id}/version")).query(&[("game_versions",format!("[\"{minecraft_version}\"]")),("loaders","[\"fabric\"]".to_string())]).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<Vec<Version>>().await.map_err(|e|e.to_string())?;let ver=versions.into_iter().next().ok_or("Brak zgodnej wersji moda")?;if ver.dependencies.iter().any(|x|x.dependency_type=="required"){return Err("Mod wymaga zależności, których instalacja automatyczna jeszcze nie obsługuje.".into())}let f=ver.files.into_iter().find(|x|x.primary).ok_or("Modrinth nie podał głównego pliku")?;if f.size>MAX_MOD_BYTES{return Err("Plik przekracza limit 512 MiB".into())}let u=reqwest::Url::parse(&f.url).map_err(|e|e.to_string())?;if u.scheme()!="https"||u.host_str()!=Some("cdn.modrinth.com"){return Err("Nieoczekiwany serwer pliku".into())}let b=c.get(u).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.bytes().await.map_err(|e|e.to_string())?;if b.len()as u64!=f.size{return Err("Rozmiar pliku nie zgadza się z manifestem".into())}let hash=hex::encode(Sha512::digest(&b));if hash!=f.hashes.sha512{return Err("Weryfikacja SHA-512 nie powiodła się".into())}let path=safe_join(&mods_dir(&state,&p)?,&f.filename).map_err(|e|e.to_string())?;fs::write(path,&b).map_err(|e|e.to_string())?;let item=Installed{project_id,file_name:f.filename,sha512:hash};let mut all=installed(&state,&p)?;all.retain(|x|x.project_id!=item.project_id);all.push(item.clone());launcher_core::store::write_json_atomic(&installed_path(&state,&p)?,&all).map_err(|e|e.to_string())?;Ok(item)}
#[tauri::command]fn remove_mod(state:tauri::State<'_,State>,profile_id:String,project_id:String)->Result<(),String>{let p=profile(&state,&profile_id)?;let mut all=installed(&state,&p)?;if let Some(i)=all.iter().position(|x|x.project_id==project_id){let path=safe_join(&mods_dir(&state,&p)?,&all[i].file_name).map_err(|e|e.to_string())?;if path.exists(){fs::remove_file(path).map_err(|e|e.to_string())?}all.remove(i);}launcher_core::store::write_json_atomic(&installed_path(&state,&p)?,&all).map_err(|e|e.to_string())}
fn main(){tauri::Builder::default().setup(|app|{let paths=AppPaths::resolve().map_err(|e|std::io::Error::other(e.to_string()))?;paths.ensure_layout().map_err(|e|std::io::Error::other(e.to_string()))?;app.manage(State{paths,active:Mutex::new(None)});Ok(())}).invoke_handler(tauri::generate_handler![app_status,list_profiles,create_profile,search_modrinth,list_installed_mods,install_mod,remove_mod,install_game_client]).run(tauri::generate_context!()).expect("failed to run Vlauncher");}
