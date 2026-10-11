Warning: fs was declared with const; use let for reassignable variables.

const state = { profiles: [], active: null, mods: [], query: "", filter: "all", account: null };
const $ = (s) => document.querySelector(s);
const esc = (s) => String(s == null ? "" : s).replace(/[&<>"']/g, (c) => ({ "&":"&amp;", "<":"&lt;", ">":"&gt;", '"':"&quot;", "'":"&#39;" }[c]));
const invoke = (name, args) => window.__TAURI__ && window.__TAURI__.core ? window.__TAURI__.core.invoke(name, args || {}) : Promise.reject("Uruchom aplikację Vlauncher.");
function toast(text) { const e=$("#toast"); e.textContent=text; e.classList.add("show"); setTimeout(()=>e.classList.remove("show"),2800); }
function showModal(title, copy) { $("#modal-title").textContent=title; $("#modal-copy").textContent=copy; $("#modal").showModal(); }
function profile() { return state.profiles.find(p=>p.id===state.active) || state.profiles[0]; }
function renderAccount() { const badge=$(".profile-banner .pill.neutral"); if(badge)badge.textContent=state.account?"KONTO: "+state.account.name:"GRA NIEPOŁĄCZONA"; }
function go(view) {
 const labels={home:"PRZEGLĄD",mods:"BIBLIOTEKA MODÓW",profiles:"PROFILE GRY",benchmarks:"BENCHMARKI",settings:"USTAWIENIA"};
 if(!labels[view]) return;
 document.querySelectorAll(".view").forEach(x=>x.classList.toggle("active-view",x.id==="view-"+view));
 document.querySelectorAll(".nav-item").forEach(x=>x.classList.toggle("active",x.dataset.view===view));
 $("#crumb-current").textContent=labels[view]; if(view==="mods") loadMods();
}
function renderProfiles() {
 const p=profile(), grid=$("#profiles-grid");
 if(!p){grid.innerHTML='<div class="empty-mods">Utwórz profil, aby rozpocząć.</div>';$("#profile-name").textContent="Brak profilu";return;}
 $("#profile-name").textContent=p.name+" · "+p.minecraftVersion; $("#active-profile-label").textContent=p.name;
 $("#profile-description").textContent=p.loader==="fabric"?"Profil Fabric. Mody są przechowywane osobno.":"Czysty profil gry.";
 grid.innerHTML=state.profiles.map(x=>'<article class="profile-card '+(x.id===p.id?'selected-profile':'')+'"><div class="profile-card-top"><span class="profile-icon">'+(x.loader==="fabric"?"✳":"◌")+'</span><span class="pill neutral">'+esc(x.loader.toUpperCase())+'</span></div><h3>'+esc(x.name)+'</h3><p>Minecraft '+esc(x.minecraftVersion)+'</p><div class="profile-card-meta"><span>'+esc(x.minecraftVersion)+'</span><span>'+esc(x.loader)+'</span></div><button class="button button-outline full-width" data-select="'+esc(x.id)+'">'+(x.id===p.id?"Aktywny profil":"Użyj profilu")+'</button><button class="button button-quiet full-width" data-game-client="'+esc(x.id)+'">Instaluj tę wersję</button>'+(x.loader==="fabric"?'<button class="button button-quiet full-width" data-fabric-loader="'+esc(x.id)+'">Instaluj Fabric</button>':"")+'</article>').join("");
 grid.querySelectorAll("[data-select]").forEach(b=>b.onclick=async()=>{state.active=b.dataset.select;renderProfiles();await loadMods();});
 grid.querySelectorAll("[data-game-client]").forEach(b=>b.onclick=async()=>{b.disabled=true;b.textContent="Pobieranie klienta…";try{const r=await invoke("install_game_client",{profileId:b.dataset.gameClient});b.textContent="Klient pobrany";toast("Pobrano "+r.versionId+": "+r.libraries+" bibliotek i "+r.assets+" zasobów. Uruchamianie gry jest w budowie.");}catch(e){b.disabled=false;b.textContent="Pobierz klienta Minecraft";showModal("Nie udało się pobrać klienta",String(e));}});
 grid.querySelectorAll("[data-fabric-loader]").forEach(b=>b.onclick=async()=>{b.disabled=true;b.textContent="Instalowanie Fabric…";try{const r=await invoke("install_fabric_loader",{profileId:b.dataset.fabricLoader});b.textContent="Fabric gotowy";toast("Fabric "+r.loaderVersion+" zainstalowany · "+r.libraries+" zweryfikowanych bibliotek.");}catch(e){b.disabled=false;b.textContent="Instaluj Fabric";showModal("Nie udało się zainstalować Fabric",String(e));}});
}
function renderHome(list) {
 list=list||[];
 $("#home-mod-list").innerHTML=list.length?list.map(x=>'<div class="mod-row"><span class="mod-art lime">M</span><span class="mod-info"><b>'+esc(x.fileName)+'</b><small>zainstalowany mod</small></span><span class="mod-version">Fabric</span><span class="mod-on">ACTIVE</span></div>').join(""):'<div class="empty-mods">Brak modów w profilu.</div>';
 $("#installed-count").textContent=String(list.length); $("#nav-mod-count").textContent=String(list.length).padStart(2,"0");
}
function renderMods() {
 const q=state.query.toLowerCase();
 const xs=state.mods.filter(x=>(state.filter!=="installed"||x.installed)&&(!q||(x.title+" "+x.author+" "+x.description).toLowerCase().includes(q)));
 $("#mods-grid").innerHTML=xs.length?xs.map(x=>'<article class="mod-card"><div class="mod-card-top"><span class="mod-art lime">'+esc((x.title||"M").slice(0,1))+'</span><span class="mod-card-name"><b>'+esc(x.title)+'</b><small>przez '+esc(x.author)+'</small></span><span class="mod-category">'+esc((x.categories||[]).slice(0,2).join(" · "))+'</span></div><p>'+esc((x.description||"").slice(0,180))+'</p><div class="mod-card-bottom"><span class="mod-version">'+Number(x.downloads||0).toLocaleString("pl-PL")+' pobrań</span><button class="install-button '+(x.installed?"installed":"")+'" data-mod="'+esc(x.project_id)+'">'+(x.installed?"✓ Zainstalowano":"+ Dodaj")+'</button></div></article>').join(""):'<div class="empty-mods">Brak wyników wyszukiwania.</div>';
 $("#all-mod-count").textContent=String(state.mods.length);$("#installed-tab-count").textContent=String(state.mods.filter(x=>x.installed).length);
}
async function loadMods() {
 const p=profile(); if(!p){$("#mods-grid").innerHTML='<div class="empty-mods">Utwórz profil Fabric, aby przeglądać mody.</div>';return;}
 if(p.loader!=="fabric"){state.mods=[];renderMods();return;}
 try {
  const pair=await Promise.all([invoke("search_modrinth",{query:state.query,gameVersion:p.minecraftVersion,loader:p.loader}),invoke("list_installed_mods",{profileId:p.id})]);
  const ids=new Set(pair[1].map(x=>x.projectId)); state.mods=pair[0].map(x=>Object.assign({},x,{installed:ids.has(x.project_id)}));
  renderMods();renderHome(pair[1]);$(".compat-banner b").textContent="Profil: Fabric · Minecraft "+p.minecraftVersion;
  $(".compat-banner small").textContent="Wyniki z Modrinth. Wymagane zależności nie są instalowane automatycznie.";
 } catch(e) { $("#mods-grid").innerHTML='<div class="empty-mods">'+esc(e)+'</div>'; }
}
document.querySelectorAll("[data-view]").forEach(b=>b.addEventListener("click",()=>go(b.dataset.view)));
document.querySelectorAll("[data-filter]").forEach(b=>b.onclick=()=>{state.filter=b.dataset.filter;document.querySelectorAll("[data-filter]").forEach(x=>x.classList.toggle("selected",x===b));renderMods();});
$("#mod-search").oninput=e=>{state.query=e.target.value;clearTimeout(state.searchTimer);state.searchTimer=setTimeout(loadMods,250);};
$("#discover-button").onclick=()=>go("mods");
$("#new-profile").onclick=async()=>{const name=prompt("Nazwa profilu:");if(!name||!name.trim())return;const version=prompt("Wersja Minecrafta:","1.21.4");if(!version)return;const loader=confirm("OK = Fabric, Anuluj = Vanilla")?"fabric":"vanilla";try{const p=await invoke("create_profile",{name:name.trim(),minecraftVersion:version.trim(),loader});state.profiles.push(p);state.active=p.id;renderProfiles();await loadMods();toast("Profil utworzony.");}catch(e){showModal("Nie udało się utworzyć profilu",String(e));}};
$("#add-profile-card").onclick=$("#new-profile").onclick;
$("#mods-grid").onclick=async e=>{const b=e.target.closest("[data-mod]");if(!b)return;const p=profile();try{if(b.classList.contains("installed"))await invoke("remove_mod",{profileId:p.id,projectId:b.dataset.mod});else await invoke("install_mod",{profileId:p.id,projectId:b.dataset.mod,minecraftVersion:p.minecraftVersion,loader:p.loader});await loadMods();toast("Lista modów zaktualizowana.");}catch(err){showModal("Nie udało się zmienić modów",String(err));}};
$("#play-button").onclick=async()=>{const p=profile();if(!p){showModal("Brak profilu","Utwórz profil Minecrafta, aby rozpocząć grę.");return;}const b=$("#play-button"),label=b.textContent;b.disabled=true;b.textContent="Uruchamianie…";try{const r=await invoke("launch_game",{profileId:p.id});b.textContent="Gra uruchomiona";toast("Minecraft uruchomiony · PID "+r.pid);setTimeout(()=>{b.disabled=false;b.textContent=label;},2500);}catch(e){b.disabled=false;b.textContent=label;showModal("Nie udało się uruchomić gry",String(e));}};
$("#run-benchmark").onclick=()=>showModal("Benchmark w budowie","Protokół pomiaru jest opisany w repozytorium. Zbieranie prawdziwych wyników FPS jeszcze nie działa.");
$("#java-button").onclick=async()=>{const p=profile(),b=$("#java-button");if(!p){showModal("Brak profilu","Utwórz profil Minecrafta, aby dobrać wersję Java.");return;}b.disabled=true;b.textContent="Pobieranie Javy…";try{const r=await invoke("install_java_runtime",{profileId:p.id});b.textContent="Java "+r.major+" gotowa";b.closest(".runtime-row").querySelector("small").textContent="Java "+r.major+" · runtime zapisany w katalogu użytkownika.";toast("Java "+r.major+" jest gotowa do użycia.");}catch(e){b.disabled=false;b.textContent="Pobierz Javę";showModal("Nie udało się przygotować Java Runtime",String(e));}};
$("#connect-game").onclick=async()=>{const clientId=prompt("Wklej publiczny identyfikator aplikacji Microsoft (GUID):");if(!clientId||!clientId.trim())return;try{const flow=await invoke("start_microsoft_login",{clientId:clientId.trim()});showModal("Zaloguj do Minecrafta","Otwórz stronę "+flow.verificationUri+" i wpisz kod "+flow.userCode+". Nie wpisuj hasła w Vlauncher. Czekam na potwierdzenie…");const poll=async()=>{try{const result=await invoke("poll_microsoft_login");if(result.pending){setTimeout(poll,Math.max(1,result.retryAfterSecs||flow.interval)*1000);return;}state.account=result.account;renderAccount();$("#modal").close();toast("Zalogowano jako "+result.account.name+".");}catch(error){showModal("Logowanie nie powiodło się",String(error));}};setTimeout(poll,flow.interval*1000);}catch(error){showModal("Nie udało się rozpocząć logowania",String(error));}};
$("#export-report").onclick=()=>showModal("Brak pomiarów","Eksport pojawi się, gdy launcher zarejestruje prawdziwe wyniki FPS.");
async function init(){try{state.account=await invoke("minecraft_account");renderAccount();const d=await invoke("list_profiles");state.profiles=d.profiles||[];state.active=d.activeProfileId||null;renderProfiles();await loadMods();}catch(e){showModal("Silnik launchera niedostępny",String(e));}}
init();
