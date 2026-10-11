use std::{fs,io::{Cursor,Read},path::PathBuf,sync::Mutex,time::Duration};
use futures_util::{stream,StreamExt};
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
#[serde(rename_all="camelCase")]
struct GameVersionInfo { id:String, downloads:std::collections::HashMap<String,GameDownload>, asset_index:AssetIndex, assets:String, #[serde(default)] libraries:Vec<GameLibrary> }
#[derive(Deserialize)]
struct GameDownload { url:String, sha1:String, size:u64 }
#[derive(Deserialize)]
#[serde(rename_all="camelCase")]
struct AssetIndex { id:String, url:String, sha1:String, size:u64 }
#[derive(Deserialize)]
struct AssetIndexData { objects:std::collections::HashMap<String,AssetObject> }
#[derive(Deserialize)]
struct AssetObject { hash:String, size:u64 }
#[derive(Deserialize)]
struct GameLibrary { downloads:Option<LibraryDownloads>, rules:Option<Vec<GameRule>>, natives:Option<std::collections::HashMap<String,String>> }
#[derive(Deserialize)]
struct LibraryDownloads { artifact:Option<GameArtifact>, classifiers:Option<std::collections::HashMap<String,GameArtifact>> }
#[derive(Deserialize)]
struct GameArtifact { path:String, url:String, sha1:String, size:u64 }
#[derive(Deserialize)]
struct GameRule { action:String, os:Option<GameOs>, features:Option<std::collections::HashMap<String,bool>> }
#[derive(Deserialize)]
struct GameOs { name:Option<String>, arch:Option<String> }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct GameClientResult { version_id:String, size:u64, client_jar:String, libraries:u32, assets:u32 }
fn valid_mojang_url(raw:&str)->Result<reqwest::Url,String>{
 let u=reqwest::Url::parse(raw).map_err(|e|e.to_string())?;
 let host=u.host_str().unwrap_or("");
 if u.scheme()!="https"||!(host=="mojang.com"||host.ends_with(".mojang.com")||host=="libraries.minecraft.net"||host=="resources.download.minecraft.net"){return Err("Nieoczekiwany adres pliku Minecrafta".into())}
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
fn library_allowed(rules:&Option<Vec<GameRule>>)->bool{
 let Some(rules)=rules else{return true};
 let mut allowed=false;
 for rule in rules {
  let os_ok=rule.os.as_ref().map(|os|os.name.as_ref().map(|n|n=="windows").unwrap_or(true)&&os.arch.as_ref().map(|a|a=="x86_64"||a=="amd64").unwrap_or(true)).unwrap_or(true);
  let features_ok=rule.features.as_ref().map(|f|f.values().all(|v|!*v)).unwrap_or(true);
  if os_ok&&features_ok{allowed=rule.action=="allow";}
 }
 allowed
}
fn write_library_file(game:&PathBuf,artifact:&GameArtifact,bytes:&[u8])->Result<PathBuf,String>{
 let dest=safe_join(&game.join("libraries"),&artifact.path).map_err(|e|e.to_string())?;
 if let Some(parent)=dest.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
 fs::write(&dest,bytes).map_err(|e|e.to_string())?;Ok(dest)
}
fn extract_native_jar(bytes:Vec<u8>,destination:&PathBuf)->Result<(),String>{
 fs::create_dir_all(destination).map_err(|e|e.to_string())?;
 let mut archive=zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e|e.to_string())?;
 for i in 0..archive.len(){
  let mut file=archive.by_index(i).map_err(|e|e.to_string())?;
  if file.is_dir()||file.name().starts_with("META-INF/"){continue}
  let Some(relative)=file.enclosed_name() else{continue};
  let name=relative.to_string_lossy().replace('\\',"/");
  if name.starts_with("META-INF/"){continue}
  let output=safe_join(destination,&name).map_err(|e|e.to_string())?;
  if let Some(parent)=output.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?;}
  let mut out=fs::File::create(output).map_err(|e|e.to_string())?;
  std::io::copy(&mut file,&mut out).map_err(|e|e.to_string())?;
 }
 Ok(())
}
\nfn id(p:&Profile)->Result<InstanceId,String>{InstanceId::parse(p.id.clone()).map_err(|e|e.to_string())}
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
 let version_dir=safe_join(&game,&format!("versions/{}",p.minecraft_version)).map_err(|e|e.to_string())?;
 fs::create_dir_all(&version_dir).map_err(|e|e.to_string())?;
 fs::write(safe_join(&version_dir,&format!("{}.json",p.minecraft_version)).map_err(|e|e.to_string())?,&meta_bytes).map_err(|e|e.to_string())?;
 let client_path=safe_join(&version_dir,&format!("{}.jar",p.minecraft_version)).map_err(|e|e.to_string())?;
 fs::write(&client_path,&jar).map_err(|e|e.to_string())?;

 let shared=state.paths.shared();
 let mut library_count=0u32;
 let natives=game.join("natives");
 for lib in &info.libraries {
  if !library_allowed(&lib.rules){continue}
  let Some(downloads)=&lib.downloads else{continue};
  if let Some(a)=&downloads.artifact {
   let dest=safe_join(&shared.join("libraries"),&a.path).map_err(|e|e.to_string())?;
   let valid=if dest.exists(){let bytes=fs::read(&dest).map_err(|e|e.to_string())?;bytes.len() as u64==a.size&&hex::encode(sha1::Sha1::digest(&bytes)).eq_ignore_ascii_case(&a.sha1)}else{false};
   if !valid{let bytes=verified_mojang_download(&c,&a.url,&a.sha1,a.size).await?;write_library_file(&shared,a,&bytes)?;}
   library_count+=1;
  }
  if let Some(native_map)=&lib.natives {
   if let Some(template)=native_map.get("windows"){
    let classifier=template.replace("${arch}","64");
    if let Some(a)=downloads.classifiers.as_ref().and_then(|m|m.get(&classifier)){
     let bytes=verified_mojang_download(&c,&a.url,&a.sha1,a.size).await?;
     write_library_file(&shared,a,&bytes)?;
     extract_native_jar(bytes,&natives)?;
    }
   }
  }
 }

 let index_bytes=verified_mojang_download(&c,&info.asset_index.url,&info.asset_index.sha1,info.asset_index.size).await?;
 let index:AssetIndexData=serde_json::from_slice(&index_bytes).map_err(|e|e.to_string())?;
 let index_root=shared.join("assets").join("indexes");
 fs::create_dir_all(&index_root).map_err(|e|e.to_string())?;
 fs::write(index_root.join(format!("{}.json",info.asset_index.id)),&index_bytes).map_err(|e|e.to_string())?;
 let objects_root=shared.join("assets").join("objects");
 let mut downloads=stream::iter(index.objects.into_iter().map(|(_,object)|{
  let client=c.clone();let root=objects_root.clone();
  async move {
   if object.hash.len()!=40{return Err("Nieprawidłowy hash zasobu Mojang".to_string())}
   let rel=format!("{}/{}",&object.hash[..2],object.hash);
   let dest=safe_join(&root,&rel).map_err(|e|e.to_string())?;
   let present=dest.metadata().map(|m|m.len()==object.size).unwrap_or(false);
   if present{return Ok(())}
   let url=format!("https://resources.download.minecraft.net/{}",rel);
   let bytes=verified_mojang_download(&client,&url,&object.hash,object.size).await?;
   if let Some(parent)=dest.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?}
   fs::write(dest,bytes).map_err(|e|e.to_string())?;
   Ok(())
  }
 })).buffer_unordered(12).collect::<Vec<Result<(),String>>>().await;
 for result in downloads.drain(..){result?}
 Ok(GameClientResult{version_id:p.minecraft_version,size:jar.len() as u64,client_jar:client_path.to_string_lossy().into_owned(),libraries:library_count,assets:index.objects.len() as u32})
}
