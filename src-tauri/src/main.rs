use std::{collections::HashMap,fs,io::{Cursor,Read},path::PathBuf,process::{Command,Stdio},sync::Mutex,time::{Duration,Instant,SystemTime,UNIX_EPOCH}};
use futures_util::{stream,StreamExt};
use serde::{Deserialize,Serialize};
use serde_json::Value;
use tauri::Manager;
use sha2::{Digest,Sha256,Sha512};
use launcher_core::paths::{AppPaths,InstanceId,safe_join};

#[derive(Clone)]
struct DeviceFlow { client_id:String, device_code:String, expires_at:Instant, interval_secs:u64 }
#[derive(Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct MinecraftAccount { id:String, name:String, client_id:String }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct DeviceCodeResult { user_code:String, verification_uri:String, expires_in:u64, interval:u64 }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct LoginPollResult { pending:bool, retry_after_secs:Option<u64>, account:Option<MinecraftAccount> }

const API:&str="https://api.modrinth.com/v2";
const MAX_MOD_BYTES:u64=536870912;
struct State{paths:AppPaths,active:Mutex<Option<String>>,device_flow:Mutex<Option<DeviceFlow>>}
#[derive(Clone,Serialize,Deserialize)]
#[serde(rename_all="camelCase")]
struct Profile{id:String,name:String,minecraft_version:String,loader:String,#[serde(default)]fabric_profile_id:Option<String>}
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
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct JavaRuntimeResult { major:u64, java_path:String, checksum:String, reused:bool }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct GameLaunchResult { pid:u32, log_file:String }
#[derive(Deserialize)]
struct FabricLibrary { name:String, url:String, sha1:String, size:u64 }
#[derive(Serialize)]
#[serde(rename_all="camelCase")]
struct FabricInstallResult { loader_version:String, profile_id:String, libraries:u32 }

fn valid_mojang_url(raw:&str)->Result<reqwest::Url,String>{
 let u=reqwest::Url::parse(raw).map_err(|e|e.to_string())?;
 let host=u.host_str().unwrap_or("");
 if u.scheme()!="https"||!(host=="mojang.com"||host.ends_with(".mojang.com")||host=="libraries.minecraft.net"||host=="resources.download.minecraft.net"){return Err("Nieoczekiwany adres pliku Minecrafta".into())}
 Ok(u)
}
async fn verified_mojang_download(c:&reqwest::Client,url:&str,sha1:&str,size:u64)->Result<Vec<u8>,String>{
 if size>536870912{return Err("Plik Minecrafta przekracza limit 512 MiB".into())}
 let response=c.get(valid_mojang_url(url)?).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
 let _=valid_mojang_url(response.url().as_str())?;
 let bytes=response.bytes().await.map_err(|e|e.to_string())?;
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

fn id(p:&Profile)->Result<InstanceId,String>{InstanceId::parse(p.id.clone()).map_err(|e|e.to_string())}
fn store_path(s:&State)->PathBuf{s.paths.root().join("profiles.json")}
fn read_store(s:&State)->Result<Store,String>{match fs::read(store_path(s)){Ok(b)=>serde_json::from_slice(&b).map_err(|e|e.to_string()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(Store{profiles:vec![],active_profile_id:None}),Err(e)=>Err(e.to_string())}}
fn write_store(s:&State,x:&Store)->Result<(),String>{launcher_core::store::write_json_atomic(&store_path(s),x).map_err(|e|e.to_string())}
fn profile(s:&State,key:&str)->Result<Profile,String>{read_store(s)?.profiles.into_iter().find(|p|p.id==key).ok_or("Nie znaleziono profilu".into())}
fn mods_dir(s:&State,p:&Profile)->Result<PathBuf,String>{let d=s.paths.instance_dir(&id(p)?).join("game").join("mods");fs::create_dir_all(&d).map_err(|e|e.to_string())?;Ok(d)}
fn installed_path(s:&State,p:&Profile)->Result<PathBuf,String>{Ok(s.paths.instance_dir(&id(p)?).join("installed-mods.json"))}
fn installed(s:&State,p:&Profile)->Result<Vec<Installed>,String>{match fs::read(installed_path(s,p)?){Ok(b)=>serde_json::from_slice(&b).map_err(|e|e.to_string()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(vec![]),Err(e)=>Err(e.to_string())}}
fn http()->Result<reqwest::Client,String>{reqwest::Client::builder().user_agent("Vlauncher/0.1 (github.com/XDP32472/Vlauncher)").timeout(Duration::from_secs(60)).build().map_err(|e|e.to_string())}

fn windows_game_rules_allow(rules:&Value)->bool {
 let Some(rules)=rules.as_array() else{return true};
 let mut allowed=false;
 for rule in rules {
  let mut matches=true;
  if let Some(os)=rule.get("os") {
   if let Some(name)=os.get("name").and_then(Value::as_str){matches &= name=="windows";}
   if let Some(arch)=os.get("arch").and_then(Value::as_str){matches &= arch=="x86_64"||arch=="amd64";}
   // Mojang's current Windows libraries do not publish version constraints.
  }
  if let Some(features)=rule.get("features").and_then(Value::as_object){
   matches &= features.values().all(|v|v.as_bool()==Some(false));
  }
  if matches {allowed=rule.get("action").and_then(Value::as_str)==Some("allow");}
 }
 allowed
}

fn expand_game_argument(value:&str,vars:&HashMap<String,String>)->Result<String,String>{
 let mut expanded=value.to_owned();
 for (key,replacement) in vars {
  let placeholder=format!("{}{{{}}}",'$',key);
  expanded=expanded.replace(&placeholder,replacement);
 }
 if expanded.contains(&format!("{}{{",'$')){return Err(format!("Nierozpoznany placeholder argumentu Minecrafta: {expanded}"))}
 Ok(expanded)
}

fn append_argument_section(metadata:&Value,section:&str,vars:&HashMap<String,String>,out:&mut Vec<String>)->Result<usize,String>{
 let Some(items)=metadata.pointer(&format!("/arguments/{section}")).and_then(Value::as_array) else{return Ok(0)};
 let mut count=0;
 for item in items {
  let value=if item.is_string(){Some(item)}else if item.get("rules").map(windows_game_rules_allow).unwrap_or(true){item.get("value")}else{None};
  let Some(value)=value else{continue};
  let values:Vec<&Value>=if let Some(array)=value.as_array(){array.iter().collect()}else{vec![value]};
  for arg in values {
   let Some(arg)=arg.as_str() else{return Err("Metadane gry zawierają argument innego typu niż tekst".into())};
   out.push(expand_game_argument(arg,vars)?);count+=1;
  }
 }
 Ok(count)
}

fn add_classpath_library(classpath:&mut Vec<String>,libraries_root:&PathBuf,relative:&str)->Result<(),String>{
 let path=safe_join(libraries_root,relative).map_err(|e|e.to_string())?;
 if !path.is_file(){return Err(format!("Brakuje biblioteki {}. Uruchom ponownie instalację klienta.",path.display()))}
 classpath.push(path.to_string_lossy().into_owned());Ok(())
}

#[tauri::command]
async fn launch_game(state:tauri::State<'_,State>,profile_id:String)->Result<GameLaunchResult,String>{
 let p=profile(&state,&profile_id)?;
 let account_path=state.paths.root().join("account.json");
 let account_bytes=fs::read(&account_path).map_err(|_|"Najpierw połącz konto Microsoft z Minecraftem.")?;
 let account:MinecraftAccount=serde_json::from_slice(&account_bytes).map_err(|e|e.to_string())?;
 let c=http()?;
 let (access_token,xuid)=refresh_minecraft_session(&c,&account).await?;
 let instance=state.paths.instance_dir(&id(&p)?);
 let game=instance.join("game");
 let version_dir=safe_join(&game,&format!("versions/{}",p.minecraft_version)).map_err(|e|e.to_string())?;
 let base_json_path=safe_join(&version_dir,&format!("{}.json",p.minecraft_version)).map_err(|e|e.to_string())?;
 let base_bytes=fs::read(&base_json_path).map_err(|_|"Najpierw pobierz klienta Minecraft dla tego profilu.")?;
 let base:Value=serde_json::from_slice(&base_bytes).map_err(|e|format!("Nieprawidłowe metadane Minecrafta: {e}"))?;
 let client_jar=safe_join(&version_dir,&format!("{}.jar",p.minecraft_version)).map_err(|e|e.to_string())?;
 if !client_jar.is_file(){return Err("Brakuje pliku klienta Minecrafta. Pobierz klienta ponownie.".into())}
 let mut fabric:Option<Value>=None;
 if p.loader=="fabric" {
  let fabric_id=p.fabric_profile_id.as_deref().ok_or("Najpierw zainstaluj Fabric dla tego profilu.")?;
  let fabric_dir=safe_join(&game,&format!("versions/{fabric_id}")).map_err(|e|e.to_string())?;
  let fabric_path=safe_join(&fabric_dir,&format!("{fabric_id}.json")).map_err(|e|e.to_string())?;
  let bytes=fs::read(&fabric_path).map_err(|_|"Nie znaleziono zainstalowanego profilu Fabric. Zainstaluj Fabric ponownie.")?;
  let metadata:Value=serde_json::from_slice(&bytes).map_err(|e|format!("Nieprawidłowy profil Fabric: {e}"))?;
  if metadata.get("inheritsFrom").and_then(Value::as_str)!=Some(p.minecraft_version.as_str()){return Err("Profil Fabric nie pasuje do wybranej wersji Minecrafta.".into())}
  fabric=Some(metadata);
 }
 let mut classpath=vec![client_jar.to_string_lossy().into_owned()];
 let shared_libraries=state.paths.shared().join("libraries");
 let parsed:GameVersionInfo=serde_json::from_value(base.clone()).map_err(|e|format!("Nieobsługiwane metadane Minecrafta: {e}"))?;
 for lib in &parsed.libraries {
  if !library_allowed(&lib.rules){continue}
  if let Some(artifact)=lib.downloads.as_ref().and_then(|x|x.artifact.as_ref()) {add_classpath_library(&mut classpath,&shared_libraries,&artifact.path)?;}
 }
 if let Some(loader)=&fabric {
  if let Some(libraries)=loader.get("libraries").and_then(Value::as_array){
   for lib in libraries {
    let name=lib.get("name").and_then(Value::as_str).ok_or("Biblioteka Fabric nie ma nazwy")?;
    add_classpath_library(&mut classpath,&shared_libraries,&fabric_library_path(name)?)?;
   }
  }
 }
 let java_major=base.pointer("/javaVersion/majorVersion").and_then(Value::as_u64).ok_or("Metadane gry nie określają wersji Java")?;
 let runtime_root=state.paths.shared().join("runtimes");
 let mut java_candidates=fs::read_dir(&runtime_root).map_err(|_|"Najpierw pobierz Java Runtime dla tego profilu.")?.filter_map(Result::ok).map(|e|e.path()).filter(|p|p.file_name().and_then(|n|n.to_str()).map(|n|n.starts_with(&format!("temurin-{java_major}-"))).unwrap_or(false)).map(|p|p.join("bin").join("java.exe")).filter(|p|p.is_file()).collect::<Vec<_>>();
 java_candidates.sort();
 let java=java_candidates.into_iter().next().ok_or("Najpierw pobierz Java Runtime dla tego profilu.")?;
 let natives=game.join("natives");
 fs::create_dir_all(&natives).map_err(|e|e.to_string())?;
 let mut vars=HashMap::new();
 let uuid=&account.id;
 let dashed=if uuid.len()==32&&uuid.bytes().all(|b|b.is_ascii_hexdigit()){format!("{}-{}-{}-{}-{}",&uuid[0..8],&uuid[8..12],&uuid[12..16],&uuid[16..20],&uuid[20..32])}else{return Err("Nieprawidłowy UUID konta Minecraft.".into())};
 let assets_root=state.paths.shared().join("assets");
 let asset_index=base.pointer("/assetIndex/id").and_then(Value::as_str).ok_or("Metadane gry nie zawierają indeksu zasobów")?;
 let version_name=p.fabric_profile_id.clone().unwrap_or_else(||p.minecraft_version.clone());
 let vars_values=[
  ("auth_player_name",account.name.clone()),("auth_uuid",dashed),("auth_access_token",access_token.clone()),("auth_session",access_token.clone()),("auth_xuid",xuid),
  ("user_type","msa".into()),("user_properties","{}".into()),("version_name",version_name),("version_type",base.get("type").and_then(Value::as_str).unwrap_or("release").to_owned()),
  ("assets_index_name",asset_index.to_owned()),("assets_root",assets_root.to_string_lossy().into_owned()),("game_directory",game.to_string_lossy().into_owned()),("natives_directory",natives.to_string_lossy().into_owned()),
  ("classpath",classpath.join(";")),("classpath_separator",";".into()),("library_directory",shared_libraries.to_string_lossy().into_owned()),("launcher_name","Vlauncher".into()),("launcher_version","0.1.0".into()),("clientid",account.client_id.clone()),
  ("resolution_width","854".into()),("resolution_height","480".into()),("quickPlayPath","".into()),("quickPlaySingleplayer","".into()),("quickPlayMultiplayer","".into()),("quickPlayRealms","".into()),
 ].into_iter().map(|(key,value)|(key.to_owned(),value)).collect::<HashMap<_,_>>();
 let mut jvm=Vec::new();
 let jvm_count=append_argument_section(&base,"jvm",&vars_values,&mut jvm)?;
 if jvm_count==0{return Err("Ta wersja Minecrafta nie obsługuje aktualnego formatu argumentów launchera.".into())}
 if let Some(loader)=&fabric{append_argument_section(loader,"jvm",&vars_values,&mut jvm)?;}
 if !jvm.iter().any(|x|x.starts_with("-Xmx")){jvm.push("-Xmx2G".into());}
 let mut game_args=Vec::new();
 if append_argument_section(&base,"game",&vars_values,&mut game_args)?==0{return Err("Metadane gry nie zawierają argumentów uruchomienia.".into())}
 if let Some(loader)=&fabric{append_argument_section(loader,"game",&vars_values,&mut game_args)?;}
 let main_class=fabric.as_ref().and_then(|x|x.get("mainClass")).or_else(||base.get("mainClass")).and_then(Value::as_str).ok_or("Metadane gry nie zawierają klasy startowej")?;
 let mut args=Vec::with_capacity(jvm.len()+game_args.len()+1);
 args.extend(jvm);args.push(main_class.to_owned());args.extend(game_args);
 let logs=state.paths.logs().join("game");fs::create_dir_all(&logs).map_err(|e|e.to_string())?;
 let stamp=SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_millis();
 let log_file=logs.join(format!("{}-{stamp}.log",p.id));
 let stdout=fs::File::create(&log_file).map_err(|e|e.to_string())?;
 let stderr=stdout.try_clone().map_err(|e|e.to_string())?;
 let child=Command::new(&java).args(&args).current_dir(&game).stdout(Stdio::from(stdout)).stderr(Stdio::from(stderr)).spawn().map_err(|e|format!("Nie udało się uruchomić Java: {e}"))?;
 Ok(GameLaunchResult{pid:child.id(),log_file:log_file.to_string_lossy().into_owned()})
}

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
 let asset_count=index.objects.len() as u32;
 let downloads=stream::iter(index.objects.into_iter().map(|(_,object)|{
  let client=c.clone();let root=objects_root.clone();
  async move {
   if object.hash.len()!=40{return Err("Nieprawidłowy hash zasobu Mojang".to_string())}
   let rel=format!("{}/{}",&object.hash[..2],object.hash);
   let dest=safe_join(&root,&rel).map_err(|e|e.to_string())?;
   let present=if dest.exists(){let cached=fs::read(&dest).map_err(|e|e.to_string())?;cached.len()as u64==object.size&&hex::encode(sha1::Sha1::digest(&cached)).eq_ignore_ascii_case(&object.hash)}else{false};
   if present{return Ok(())}
   let url=format!("https://resources.download.minecraft.net/{}",rel);
   let bytes=verified_mojang_download(&client,&url,&object.hash,object.size).await?;
   if let Some(parent)=dest.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?}
   fs::write(dest,bytes).map_err(|e|e.to_string())?;
   Ok(())
  }
 })).buffer_unordered(12).collect::<Vec<Result<(),String>>>().await;
 for result in downloads{result?}
 Ok(GameClientResult{version_id:p.minecraft_version,size:jar.len() as u64,client_jar:client_path.to_string_lossy().into_owned(),libraries:library_count,assets:asset_count})
}




#[tauri::command]
async fn install_java_runtime(state:tauri::State<'_,State>,profile_id:String)->Result<JavaRuntimeResult,String>{
 let p=profile(&state,&profile_id)?;
 let c=http()?;
 let manifest=c.get("https://piston-meta.mojang.com/mc/game/version_manifest_v2.json").send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<GameManifest>().await.map_err(|e|e.to_string())?;
 let entry=manifest.versions.into_iter().find(|v|v.id==p.minecraft_version).ok_or("Nie znaleziono tej wersji w manifeście Mojang")?;
 let meta=c.get(valid_mojang_url(&entry.url)?).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
 let _=valid_mojang_url(meta.url().as_str())?;
 let meta_bytes=meta.bytes().await.map_err(|e|e.to_string())?;
 if !hex::encode(sha1::Sha1::digest(&meta_bytes)).eq_ignore_ascii_case(&entry.sha1){return Err("Weryfikacja metadanych Minecrafta nie powiodła się".into())}
 let info:serde_json::Value=serde_json::from_slice(&meta_bytes).map_err(|e|e.to_string())?;
 let major=info.pointer("/javaVersion/majorVersion").and_then(|x|x.as_u64()).ok_or("Metadane Minecrafta nie określają wymaganej wersji Java")?;
 if !(8..=99).contains(&major){return Err("Wersja Java wymagana przez Minecrafta jest nieobsługiwana".into())}
 let api=format!("https://api.adoptium.net/v3/assets/latest/{major}/hotspot?architecture=x64&image_type=jre&os=windows&vendor=eclipse");
 let assets=c.get(api).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<Vec<serde_json::Value>>().await.map_err(|e|e.to_string())?;
 let release=assets.first().ok_or("Adoptium nie udostępnia pasującego runtime Java")?;
 let binary=release.get("binary").ok_or("Brak informacji o wydaniu Java")?;
 if binary.get("architecture").and_then(|x|x.as_str())!=Some("x64")||binary.get("os").and_then(|x|x.as_str())!=Some("windows")||binary.get("image_type").and_then(|x|x.as_str())!=Some("jre")||binary.get("jvm_impl").and_then(|x|x.as_str())!=Some("hotspot"){
  return Err("Adoptium zwróciło niezgodny runtime Java".into())
 }
 let package=binary.get("package").ok_or("Brak paczki runtime Java")?;
 let checksum=package.get("checksum").and_then(|x|x.as_str()).ok_or("Brak sumy SHA-256 runtime Java")?.to_ascii_lowercase();
 if checksum.len()!=64||!checksum.bytes().all(|b|b.is_ascii_hexdigit()){return Err("Nieprawidłowa suma SHA-256 runtime Java".into())}
 let size=package.get("size").and_then(|x|x.as_u64()).ok_or("Brak rozmiaru runtime Java")?;
 if size>536870912{return Err("Paczka runtime Java przekracza limit 512 MiB".into())}
 let download=package.get("link").and_then(|x|x.as_str()).ok_or("Brak adresu runtime Java")?;
 let url=reqwest::Url::parse(download).map_err(|e|e.to_string())?;
 if url.scheme()!="https"||url.host_str()!=Some("github.com"){return Err("Nieoczekiwany serwer runtime Java".into())}
 let response=c.get(url).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
 if response.url().scheme()!="https"||!matches!(response.url().host_str(),Some("github.com"|"release-assets.githubusercontent.com"|"objects.githubusercontent.com")){
  return Err("Nieoczekiwane przekierowanie runtime Java".into())
 }
 let bytes=response.bytes().await.map_err(|e|e.to_string())?;
 if bytes.len()as u64!=size||hex::encode(Sha256::digest(&bytes))!=checksum{return Err("Weryfikacja SHA-256 runtime Java nie powiodła się".into())}
 let root=state.paths.shared().join("runtimes");
 fs::create_dir_all(&root).map_err(|e|e.to_string())?;
 let runtime_name=format!("temurin-{major}-{}",&checksum[..12]);
 let target=root.join(&runtime_name);
 let java_path=target.join("bin").join("java.exe");
 if java_path.is_file(){
  let marker=target.join(".vlauncher-runtime.json");
  if let Ok(data)=fs::read(&marker){if let Ok(saved)=serde_json::from_slice::<serde_json::Value>(&data){
   if saved.get("checksum").and_then(|x|x.as_str())==Some(checksum.as_str()){return Ok(JavaRuntimeResult{major,java_path:java_path.to_string_lossy().into_owned(),checksum,reused:true})}
  }}
  return Err("Istnieje runtime Java bez zgodnego znacznika Vlauncher; wybierz inny katalog danych".into())
 }
 let staging=root.join(format!(".{runtime_name}-staging-{}",uuid::Uuid::new_v4().simple()));
 fs::create_dir_all(&staging).map_err(|e|e.to_string())?;
 let mut archive=zip::ZipArchive::new(Cursor::new(bytes)).map_err(|e|e.to_string())?;
 if archive.len()>50000{return Err("Paczka Java zawiera zbyt wiele plików".into())}
 let mut expanded=0u64;
 for i in 0..archive.len(){
  let mut file=archive.by_index(i).map_err(|e|e.to_string())?;
  if file.unix_mode().is_some_and(|mode|mode&0o170000==0o120000){return Err("Paczka Java zawiera dowiązanie symboliczne".into())}
  expanded=expanded.saturating_add(file.size());
  if expanded>1073741824{return Err("Rozpakowana paczka Java przekracza limit 1 GiB".into())}
  let Some(relative)=file.enclosed_name() else{continue};
  let mut components=relative.components();
  let Some(first)=components.next() else{continue};
  let Some(child)=relative.strip_prefix(first.as_os_str()).ok() else{continue};
  if child.as_os_str().is_empty(){continue}
  let child=child.to_string_lossy().replace('\\',"/");
  let output=safe_join(&staging,&child).map_err(|e|e.to_string())?;
  if file.is_dir(){fs::create_dir_all(output).map_err(|e|e.to_string())?;continue}
  if let Some(parent)=output.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?}
  let mut out=fs::File::create(output).map_err(|e|e.to_string())?;
  std::io::copy(&mut file,&mut out).map_err(|e|e.to_string())?;
 }
 let staged_java=staging.join("bin").join("java.exe");
 if !staged_java.is_file(){return Err("Nie znaleziono java.exe w paczce runtime".into())}
 fs::write(staging.join(".vlauncher-runtime.json"),serde_json::to_vec(&serde_json::json!({"major":major,"checksum":checksum,"vendor":"Eclipse Temurin"})).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
 fs::rename(&staging,&target).map_err(|e|e.to_string())?;
 Ok(JavaRuntimeResult{major,java_path:java_path.to_string_lossy().into_owned(),checksum,reused:false})
}

fn fabric_library_path(coordinate:&str)->Result<String,String>{
 let parts:Vec<_>=coordinate.split(':').collect();
 if !(3..=4).contains(&parts.len()){return Err("Nieprawidłowe współrzędne biblioteki Fabric".into())}
 let group=parts[0];let artifact=parts[1];let version_ext=parts[2];
 if group.is_empty()||artifact.is_empty()||version_ext.is_empty()
   ||!group.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'_')
   ||!artifact.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'_'||b==b'-'){
  return Err("Nieprawidłowe współrzędne biblioteki Fabric".into())
 }
 let (version,extension)=version_ext.split_once('@').unwrap_or((version_ext,"jar"));
 if !extension.eq_ignore_ascii_case("jar")||!version.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'_'||b==b'-'||b==b'+'){
  return Err("Nieobsługiwany typ biblioteki Fabric".into())
 }
 let classifier=if parts.len()==4{format!("-{}",parts[3])}else{String::new()};
 if parts.len()==4&&!parts[3].bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'_'||b==b'-'){
  return Err("Nieprawidłowa klasyfikacja biblioteki Fabric".into())
 }
 Ok(format!("{}/{}/{}/{}-{}{}.jar",group.replace('.', "/"),artifact,version,artifact,version,classifier))
}
#[tauri::command]
async fn install_fabric_loader(state:tauri::State<'_,State>,profile_id:String)->Result<FabricInstallResult,String>{
 let p=profile(&state,&profile_id)?;
 if p.loader!="fabric"{return Err("Fabric można zainstalować tylko w profilu Fabric".into())}
 let c=http()?;
 let list_url=format!("https://meta.fabricmc.net/v2/versions/loader/{}",p.minecraft_version);
 let versions=c.get(list_url).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<Vec<serde_json::Value>>().await.map_err(|e|e.to_string())?;
 let selected=versions.iter().find(|v|v.pointer("/loader/stable").and_then(|x|x.as_bool())==Some(true)).or_else(||versions.first()).ok_or("Fabric nie udostępnia loadera dla tej wersji Minecrafta")?;
 let loader_version=selected.pointer("/loader/version").and_then(|x|x.as_str()).ok_or("Metadane Fabric nie zawierają wersji loadera")?.to_owned();
 if loader_version.len()>64||!loader_version.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'.'||b==b'-'||b==b'_'||b==b'+'){
  return Err("Fabric zwrócił nieprawidłowy numer wersji".into())
 }
 let profile_url=format!("https://meta.fabricmc.net/v2/versions/loader/{}/{}/profile/json",p.minecraft_version,loader_version);
 let profile_json=c.get(profile_url).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<serde_json::Value>().await.map_err(|e|e.to_string())?;
 let loader_id=profile_json.get("id").and_then(|x|x.as_str()).ok_or("Profil Fabric nie zawiera identyfikatora")?.to_owned();
 if loader_id.len()>128||!loader_id.bytes().all(|b|b.is_ascii_alphanumeric()||b==b'-'||b==b'_'||b==b'.'){
  return Err("Profil Fabric zawiera nieprawidłowy identyfikator".into())
 }
 if profile_json.get("inheritsFrom").and_then(|x|x.as_str())!=Some(p.minecraft_version.as_str()){
  return Err("Profil Fabric wskazuje inną wersję Minecrafta".into())
 }
 let libs=serde_json::from_value::<Vec<FabricLibrary>>(profile_json.get("libraries").cloned().ok_or("Profil Fabric nie zawiera bibliotek")?).map_err(|e|e.to_string())?;
 let root=state.paths.shared().join("libraries");
 for lib in &libs{
  if lib.size>MAX_MOD_BYTES{return Err("Biblioteka Fabric przekracza limit 512 MiB".into())}
  let relative=fabric_library_path(&lib.name)?;
  let base=reqwest::Url::parse(&lib.url).map_err(|e|e.to_string())?;
  if base.scheme()!="https"||base.host_str()!=Some("maven.fabricmc.net")||!base.path().ends_with('/'){
   return Err("Nieoczekiwany serwer biblioteki Fabric".into())
  }
  let url=base.join(&relative).map_err(|e|e.to_string())?;
  let dest=safe_join(&root,&relative).map_err(|e|e.to_string())?;
  let present=if dest.exists(){let bytes=fs::read(&dest).map_err(|e|e.to_string())?;bytes.len()as u64==lib.size&&hex::encode(sha1::Sha1::digest(&bytes)).eq_ignore_ascii_case(&lib.sha1)}else{false};
  if !present{
   let response=c.get(url).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;
   if response.url().scheme()!="https"||response.url().host_str()!=Some("maven.fabricmc.net"){return Err("Nieoczekiwane przekierowanie biblioteki Fabric".into())}
   let bytes=response.bytes().await.map_err(|e|e.to_string())?;
   if bytes.len()as u64!=lib.size||!hex::encode(sha1::Sha1::digest(&bytes)).eq_ignore_ascii_case(&lib.sha1){return Err(format!("Weryfikacja biblioteki Fabric {} nie powiodła się",lib.name))}
   if let Some(parent)=dest.parent(){fs::create_dir_all(parent).map_err(|e|e.to_string())?}
   fs::write(dest,&bytes).map_err(|e|e.to_string())?;
  }
 }
 let game=mods_dir(&state,&p)?.parent().ok_or("Nie można ustalić katalogu gry")?.to_path_buf();
 let version_dir=safe_join(&game,&format!("versions/{}",loader_id)).map_err(|e|e.to_string())?;
 fs::create_dir_all(&version_dir).map_err(|e|e.to_string())?;
 let metadata=safe_join(&version_dir,&format!("{}.json",loader_id)).map_err(|e|e.to_string())?;
 fs::write(metadata,serde_json::to_vec_pretty(&profile_json).map_err(|e|e.to_string())?).map_err(|e|e.to_string())?;
 let mut saved=read_store(&state)?;
 let selected=saved.profiles.iter_mut().find(|x|x.id==p.id).ok_or("Nie znaleziono profilu po instalacji Fabric")?;
 selected.fabric_profile_id=Some(loader_id.clone());
 write_store(&state,&saved)?;
 Ok(FabricInstallResult{loader_version,profile_id:loader_id,libraries:libs.len()as u32})
}


async fn auth_json(c:&reqwest::Client,url:&str,payload:serde_json::Value,label:&str)->Result<serde_json::Value,String>{
 let response=c.post(url).json(&payload).send().await.map_err(|e|e.to_string())?;
 let status=response.status();
 let body=response.json::<serde_json::Value>().await.map_err(|e|format!("{label}: nieprawidłowa odpowiedź"))?;
 if !status.is_success(){return Err(format!("{label}: serwer odrzucił żądanie (HTTP {})",status.as_u16()))}
 Ok(body)
}
#[cfg(windows)]
fn load_refresh_token(account_id:&str)->Result<String,String>{
 keyring::Entry::new("com.xdp32472.vlauncher",account_id).map_err(|e|e.to_string())?.get_password().map_err(|_|"Brakuje bezpiecznego tokenu logowania. Połącz ponownie konto Microsoft.".into())
}
#[cfg(not(windows))]
fn load_refresh_token(_account_id:&str)->Result<String,String>{Err("Logowanie dostępne jest obecnie tylko w Windows.".into())}

async fn refresh_minecraft_session(c:&reqwest::Client,account:&MinecraftAccount)->Result<(String,String),String>{
 let refresh=load_refresh_token(&account.id)?;
 let response=c.post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
  .form(&[("grant_type","refresh_token"),("client_id",account.client_id.as_str()),("refresh_token",refresh.as_str()),("scope","XboxLive.signin offline_access")])
  .send().await.map_err(|e|e.to_string())?;
 let status=response.status();
 let token=response.json::<Value>().await.map_err(|_|"Microsoft zwrócił nieprawidłową odpowiedź logowania".to_string())?;
 if !status.is_success(){return Err(token.get("error_description").and_then(Value::as_str).unwrap_or("Nie udało się odświeżyć logowania Microsoft").to_owned())}
 let ms_access=token.get("access_token").and_then(Value::as_str).ok_or("Microsoft nie zwrócił tokenu dostępu")?;
 if let Some(rotated)=token.get("refresh_token").and_then(Value::as_str){save_refresh_token(&account.id,rotated)?;}
 let xbox=auth_json(c,"https://user.auth.xboxlive.com/user/authenticate",serde_json::json!({"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={ms_access}")},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"}),"Xbox Live").await?;
 let xbox_token=xbox.get("Token").and_then(Value::as_str).ok_or("Xbox Live nie zwrócił tokenu")?;
 let xsts=auth_json(c,"https://xsts.auth.xboxlive.com/xsts/authorize",serde_json::json!({"Properties":{"SandboxId":"RETAIL","UserTokens":[xbox_token]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"}),"Xbox Security Token Service").await?;
 let xsts_token=xsts.get("Token").and_then(Value::as_str).ok_or("Xbox Security Token Service nie zwrócił tokenu")?;
 let user_hash=xsts.pointer("/DisplayClaims/xui/0/uhs").and_then(Value::as_str).ok_or("Xbox nie zwrócił identyfikatora użytkownika")?;
 let xuid=xsts.pointer("/DisplayClaims/xui/0/xid").and_then(Value::as_str).unwrap_or("").to_owned();
 let minecraft=auth_json(c,"https://api.minecraftservices.com/authentication/login_with_xbox",serde_json::json!({"identityToken":format!("XBL3.0 x={user_hash};{xsts_token}")}),"Minecraft").await?;
 let minecraft_token=minecraft.get("access_token").and_then(Value::as_str).ok_or("Minecraft nie zwrócił tokenu dostępu")?.to_owned();
 Ok((minecraft_token,xuid))
}
#[tauri::command]
async fn start_microsoft_login(state:tauri::State<'_,State>,client_id:String)->Result<DeviceCodeResult,String>{
 let client_id=client_id.trim().to_owned();
 if client_id.len()!=36||!client_id.bytes().enumerate().all(|(i,b)|if [8,13,18,23].contains(&i){b==b'-'}else{b.is_ascii_hexdigit()}){
  return Err("Podaj identyfikator aplikacji Microsoft w formacie GUID".into())
 }
 let c=http()?;
 let response=c.post("https://login.microsoftonline.com/consumers/oauth2/v2.0/devicecode")
  .form(&[("client_id",client_id.as_str()),("scope","XboxLive.signin offline_access")])
  .send().await.map_err(|e|e.to_string())?;
 let status=response.status();
 let body=response.json::<serde_json::Value>().await.map_err(|e|e.to_string())?;
 if !status.is_success(){return Err(body.get("error_description").and_then(|x|x.as_str()).unwrap_or("Microsoft odrzucił żądanie logowania").to_owned())}
 let device_code=body.get("device_code").and_then(|x|x.as_str()).ok_or("Microsoft nie zwrócił kodu urządzenia")?.to_owned();
 let user_code=body.get("user_code").and_then(|x|x.as_str()).ok_or("Microsoft nie zwrócił kodu użytkownika")?.to_owned();
 let verification_uri=body.get("verification_uri").and_then(|x|x.as_str()).or_else(||body.get("verification_url").and_then(|x|x.as_str())).ok_or("Microsoft nie podał strony logowania")?.to_owned();
 let expires_in=body.get("expires_in").and_then(|x|x.as_u64()).unwrap_or(900).clamp(60,1800);
 let interval=body.get("interval").and_then(|x|x.as_u64()).unwrap_or(5).clamp(1,30);
 *state.device_flow.lock().map_err(|e|e.to_string())?=Some(DeviceFlow{client_id,device_code,expires_at:Instant::now()+Duration::from_secs(expires_in),interval_secs:interval});
 Ok(DeviceCodeResult{user_code,verification_uri,expires_in,interval})
}
#[tauri::command]
async fn poll_microsoft_login(state:tauri::State<'_,State>)->Result<LoginPollResult,String>{
 let flow=state.device_flow.lock().map_err(|e|e.to_string())?.clone().ok_or("Nie rozpoczęto logowania Microsoft")?;
 if Instant::now()>=flow.expires_at{
  *state.device_flow.lock().map_err(|e|e.to_string())?=None;
  return Err("Kod logowania wygasł. Rozpocznij ponownie.".into())
 }
 let c=http()?;
 let response=c.post("https://login.microsoftonline.com/consumers/oauth2/v2.0/token")
  .form(&[("grant_type","urn:ietf:params:oauth:grant-type:device_code"),("client_id",flow.client_id.as_str()),("device_code",flow.device_code.as_str())])
  .send().await.map_err(|e|e.to_string())?;
 let status=response.status();
 let token=response.json::<serde_json::Value>().await.map_err(|e|e.to_string())?;
 if !status.is_success(){
  match token.get("error").and_then(|x|x.as_str()).unwrap_or("") {
   "authorization_pending"=>return Ok(LoginPollResult{pending:true,retry_after_secs:Some(flow.interval_secs),account:None}),
   "slow_down"=>{
    if let Some(mut current)=state.device_flow.lock().map_err(|e|e.to_string())?.as_mut(){current.interval_secs=(current.interval_secs+5).min(30);}
    return Ok(LoginPollResult{pending:true,retry_after_secs:Some(flow.interval_secs+5),account:None})
   },
   "authorization_declined"|"expired_token"=>{
    *state.device_flow.lock().map_err(|e|e.to_string())?=None;
    return Err("Logowanie Microsoft zostało anulowane lub wygasło.".into())
   },
   _=>return Err(token.get("error_description").and_then(|x|x.as_str()).unwrap_or("Logowanie Microsoft nie powiodło się").to_owned())
  }
 }
 let access=token.get("access_token").and_then(|x|x.as_str()).ok_or("Microsoft nie zwrócił tokenu dostępu")?;
 let refresh=token.get("refresh_token").and_then(|x|x.as_str()).ok_or("Microsoft nie zwrócił tokenu odświeżania; wymagane jest uprawnienie offline_access")?;
 let xbox=auth_json(&c,"https://user.auth.xboxlive.com/user/authenticate",serde_json::json!({"Properties":{"AuthMethod":"RPS","SiteName":"user.auth.xboxlive.com","RpsTicket":format!("d={access}")},"RelyingParty":"http://auth.xboxlive.com","TokenType":"JWT"}),"Xbox Live").await?;
 let xbox_token=xbox.get("Token").and_then(|x|x.as_str()).ok_or("Xbox Live nie zwrócił tokenu")?;
 let user_hash=xbox.pointer("/DisplayClaims/xui/0/uhs").and_then(|x|x.as_str()).ok_or("Xbox Live nie zwrócił identyfikatora użytkownika")?;
 let xsts=auth_json(&c,"https://xsts.auth.xboxlive.com/xsts/authorize",serde_json::json!({"Properties":{"SandboxId":"RETAIL","UserTokens":[xbox_token]},"RelyingParty":"rp://api.minecraftservices.com/","TokenType":"JWT"}),"Xbox Security Token Service").await?;
 let xsts_token=xsts.get("Token").and_then(|x|x.as_str()).ok_or("Xbox Security Token Service nie zwrócił tokenu")?;
 let minecraft=auth_json(&c,"https://api.minecraftservices.com/authentication/login_with_xbox",serde_json::json!({"identityToken":format!("XBL3.0 x={user_hash};{xsts_token}")}),"Minecraft").await?;
 let minecraft_token=minecraft.get("access_token").and_then(|x|x.as_str()).ok_or("Minecraft nie zwrócił tokenu")?;
 let entitlements=c.get("https://api.minecraftservices.com/entitlements/mcstore").bearer_auth(minecraft_token).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|_|"Nie udało się sprawdzić licencji Minecrafta")?.json::<serde_json::Value>().await.map_err(|e|e.to_string())?;
 let owns_game=entitlements.get("items").and_then(|x|x.as_array()).map(|items|items.iter().any(|item|matches!(item.get("name").and_then(|x|x.as_str()),Some("game_minecraft"|"product_minecraft")))).unwrap_or(false);
 if !owns_game{return Err("To konto Microsoft nie ma licencji Minecraft: Java Edition.".into())}
 let profile_response=c.get("https://api.minecraftservices.com/minecraft/profile").bearer_auth(minecraft_token).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|_|"Konto nie ma dostępnego profilu Minecraft: Java Edition")?.json::<serde_json::Value>().await.map_err(|e|e.to_string())?;
 let id=profile_response.get("id").and_then(|x|x.as_str()).ok_or("Minecraft nie zwrócił UUID profilu")?.to_owned();
 let name=profile_response.get("name").and_then(|x|x.as_str()).ok_or("Minecraft nie zwrócił nazwy profilu")?.to_owned();
 if id.len()!=32||!id.bytes().all(|b|b.is_ascii_hexdigit()){return Err("Minecraft zwrócił nieprawidłowy UUID".into())}
 save_refresh_token(&id,refresh)?;
 let account=MinecraftAccount{id,name,client_id:flow.client_id};
 launcher_core::store::write_json_atomic(&state.paths.root().join("account.json"),&account).map_err(|e|e.to_string())?;
 *state.device_flow.lock().map_err(|e|e.to_string())?=None;
 Ok(LoginPollResult{pending:false,retry_after_secs:None,account:Some(account)})
}
#[cfg(windows)]
fn save_refresh_token(account_id:&str,token:&str)->Result<(),String>{
 keyring::Entry::new("com.xdp32472.vlauncher",account_id).map_err(|e|e.to_string())?.set_password(token).map_err(|e|e.to_string())
}
#[cfg(not(windows))]
fn save_refresh_token(_account_id:&str,_token:&str)->Result<(),String>{
 Err("Zapisywanie bezpiecznych tokenów jest obecnie dostępne tylko w Windows.".into())
}
#[tauri::command]
fn minecraft_account(state:tauri::State<'_,State>)->Result<Option<MinecraftAccount>,String>{
 let path=state.paths.root().join("account.json");
 match fs::read(path){Ok(bytes)=>serde_json::from_slice(&bytes).map(Some).map_err(|e|e.to_string()),Err(e)if e.kind()==std::io::ErrorKind::NotFound=>Ok(None),Err(e)=>Err(e.to_string())}
}

#[tauri::command]fn app_status()->String{"Vlauncher 0.1 · profile and mod management".into()}
#[tauri::command]fn list_profiles(state:tauri::State<'_,State>)->Result<ProfileResult,String>{let s=read_store(&state)?;let active=state.active.lock().map_err(|e|e.to_string())?.clone().or(s.active_profile_id).or_else(||s.profiles.first().map(|p|p.id.clone()));Ok(ProfileResult{profiles:s.profiles,active_profile_id:active})}
#[tauri::command]fn create_profile(state:tauri::State<'_,State>,name:String,minecraft_version:String,loader:String)->Result<Profile,String>{let name=name.trim();let v=minecraft_version.trim();let l=loader.to_lowercase();if name.is_empty()||name.len()>48{return Err("Nazwa profilu musi mieć od 1 do 48 znaków".into())}if v.is_empty()||v.len()>32||!v.chars().all(|c|c.is_ascii_alphanumeric()||c=='.'||c=='-'){return Err("Nieprawidłowa wersja Minecrafta".into())}if l!="fabric"&&l!="vanilla"{return Err("Obsługiwane profile: Fabric lub Vanilla".into())}let p=Profile{id:uuid::Uuid::new_v4().simple().to_string(),name:name.into(),minecraft_version:v.into(),loader:l,fabric_profile_id:None};let mut s=read_store(&state)?;s.profiles.push(p.clone());s.active_profile_id=Some(p.id.clone());write_store(&state,&s)?;*state.active.lock().map_err(|e|e.to_string())?=Some(p.id.clone());fs::create_dir_all(mods_dir(&state,&p)?).map_err(|e|e.to_string())?;Ok(p)}
#[tauri::command]async fn search_modrinth(query:String,game_version:String,loader:String)->Result<Vec<Hit>,String>{let facets=serde_json::json!([["project_type:mod"],[format!("versions:{game_version}")],[format!("categories:{loader}")]]).to_string();let res=http()?.get(format!("{API}/search")).query(&[("query",query.as_str()),("facets",facets.as_str()),("limit","40")]).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<serde_json::Value>().await.map_err(|e|e.to_string())?;serde_json::from_value(res["hits"].clone()).map_err(|e|e.to_string())}
#[tauri::command]fn list_installed_mods(state:tauri::State<'_,State>,profile_id:String)->Result<Vec<Installed>,String>{let p=profile(&state,&profile_id)?;let _=mods_dir(&state,&p)?;installed(&state,&p)}
#[tauri::command]async fn install_mod(state:tauri::State<'_,State>,profile_id:String,project_id:String,minecraft_version:String,loader:String)->Result<Installed,String>{let p=profile(&state,&profile_id)?;if p.loader!="fabric"||loader!="fabric"{return Err("Mody są obecnie dostępne tylko dla profili Fabric".into())}if project_id.len()>64||!project_id.chars().all(|c|c.is_ascii_alphanumeric()||c=='_'||c=='-'){return Err("Nieprawidłowy identyfikator moda".into())}let c=http()?;let versions=c.get(format!("{API}/project/{project_id}/version")).query(&[("game_versions",format!("[\"{minecraft_version}\"]")),("loaders","[\"fabric\"]".to_string())]).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?.json::<Vec<Version>>().await.map_err(|e|e.to_string())?;let ver=versions.into_iter().next().ok_or("Brak zgodnej wersji moda")?;if ver.dependencies.iter().any(|x|x.dependency_type=="required"){return Err("Mod wymaga zależności, których instalacja automatyczna jeszcze nie obsługuje.".into())}let f=ver.files.into_iter().find(|x|x.primary).ok_or("Modrinth nie podał głównego pliku")?;if f.size>MAX_MOD_BYTES{return Err("Plik przekracza limit 512 MiB".into())}let u=reqwest::Url::parse(&f.url).map_err(|e|e.to_string())?;if u.scheme()!="https"||u.host_str()!=Some("cdn.modrinth.com"){return Err("Nieoczekiwany serwer pliku".into())}let response=c.get(u).send().await.map_err(|e|e.to_string())?.error_for_status().map_err(|e|e.to_string())?;if response.url().scheme()!="https"||response.url().host_str()!=Some("cdn.modrinth.com"){return Err("Nieoczekiwane przekierowanie pliku".into())}let b=response.bytes().await.map_err(|e|e.to_string())?;if b.len()as u64!=f.size{return Err("Rozmiar pliku nie zgadza się z manifestem".into())}let hash=hex::encode(Sha512::digest(&b));if hash!=f.hashes.sha512{return Err("Weryfikacja SHA-512 nie powiodła się".into())}let path=safe_join(&mods_dir(&state,&p)?,&f.filename).map_err(|e|e.to_string())?;fs::write(path,&b).map_err(|e|e.to_string())?;let item=Installed{project_id,file_name:f.filename,sha512:hash};let mut all=installed(&state,&p)?;all.retain(|x|x.project_id!=item.project_id);all.push(item.clone());launcher_core::store::write_json_atomic(&installed_path(&state,&p)?,&all).map_err(|e|e.to_string())?;Ok(item)}
#[tauri::command]fn remove_mod(state:tauri::State<'_,State>,profile_id:String,project_id:String)->Result<(),String>{let p=profile(&state,&profile_id)?;let mut all=installed(&state,&p)?;if let Some(i)=all.iter().position(|x|x.project_id==project_id){let path=safe_join(&mods_dir(&state,&p)?,&all[i].file_name).map_err(|e|e.to_string())?;if path.exists(){fs::remove_file(path).map_err(|e|e.to_string())?}all.remove(i);}launcher_core::store::write_json_atomic(&installed_path(&state,&p)?,&all).map_err(|e|e.to_string())}
 fn main(){tauri::Builder::default().setup(|app|{let paths=AppPaths::resolve().map_err(|e|std::io::Error::other(e.to_string()))?;paths.ensure_layout().map_err(|e|std::io::Error::other(e.to_string()))?;app.manage(State{paths,active:Mutex::new(None),device_flow:Mutex::new(None)});Ok(())}).invoke_handler(tauri::generate_handler![app_status,list_profiles,create_profile,search_modrinth,list_installed_mods,install_mod,remove_mod,install_game_client,install_fabric_loader,install_java_runtime,launch_game,start_microsoft_login,poll_microsoft_login,minecraft_account]).run(tauri::generate_context!()).expect("failed to run Vlauncher");}
