//! Layout, componentes y páginas de empresa. Todo texto sale del catálogo de idiomas (`catalog.rs`):
//! ninguna vista contiene texto visible escrito a mano, salvo nombres propios y datos de las fuentes.

use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::analysis::{self, Signal};
use crate::annual_report::Financials;
use crate::bolagsverket::Organisation;
use crate::db::{Role, User};
use crate::format::js_round;
use crate::i18n::{self, Lang};
use crate::model::*;
use crate::registry::Hit;
use crate::scb::SectorMedians;
use crate::summary::generate_example_summary;
use crate::util::{query_without, urlencode};

// ───────────────────────── Contexto de la petición ─────────────────────────

/// Lo que toda vista necesita saber: idioma, usuario, ruta actual y token CSRF.
#[derive(Clone)]
pub struct Ctx {
    pub lang: Lang,
    pub user: Option<User>,
    pub path: String,
    pub query: String,
    pub csrf: String,
    /// Modo demo: muestra las empresas y pantallas de EJEMPLO.
    pub demo: bool,
}

impl Ctx {
    #[cfg(test)]
    pub fn new(lang: Lang) -> Ctx {
        Ctx { lang, user: None, path: "/".into(), query: String::new(), csrf: String::new(), demo: true }
    }
    pub fn t(&self, key: &str) -> &'static str {
        i18n::t(self.lang, key)
    }
    pub fn tf(&self, key: &str, args: &[&str]) -> String {
        i18n::tf(self.lang, key, args)
    }
    pub fn int(&self, n: i64) -> String {
        i18n::int(self.lang, n)
    }
    pub fn pct1(&self, v: f64) -> String {
        i18n::pct1(self.lang, v)
    }
    pub fn pct(&self, v: f64) -> String {
        i18n::pct(self.lang, v)
    }
    pub fn num(&self, v: f64) -> String {
        i18n::num(self.lang, v)
    }
    pub fn dec(&self, v: f64, d: usize) -> String {
        i18n::dec(self.lang, v, d)
    }
    /// "64 100 tkr" / "64,100 kSEK" / "64.100 mil SEK"
    pub fn money(&self, tkr: i64) -> String {
        format!("{} {}", self.int(tkr), self.t("unit.tkr"))
    }
    pub fn csrf_input(&self) -> Markup {
        html! { input type="hidden" name="csrf" value=(self.csrf); }
    }
    /// Misma página con otro idioma (conserva el resto de la query).
    pub fn url_for_lang(&self, l: Lang) -> String {
        let rest = query_without(&self.query, "lang");
        if rest.is_empty() { format!("{}?lang={}", self.path, l.code()) } else { format!("{}?{rest}&lang={}", self.path, l.code()) }
    }
}

// ───────────────────────── Iconos (trazo único: 1.75, redondeado) ─────────────────────────

pub fn icon(name: &str) -> Markup {
    icon_sized(name, "icon")
}

pub fn icon_sized(name: &str, class: &str) -> Markup {
    let paths = match name {
        "search" => r#"<circle cx="11" cy="11" r="7"/><path d="m20 20-3.5-3.5"/>"#,
        "bell" => r#"<path d="M6 8a6 6 0 0 1 12 0c0 7 3 9 3 9H3s3-2 3-9"/><path d="M10.3 21a1.94 1.94 0 0 0 3.4 0"/>"#,
        "trend" => r#"<path d="M3 3v16a2 2 0 0 0 2 2h16"/><path d="m19 9-5 5-4-4-3 3"/>"#,
        "upload" => r#"<path d="M12 15V3"/><path d="m7 8 5-5 5 5"/><path d="M5 21h14"/>"#,
        "receipt" => r#"<path d="M4 2v20l2-1 2 1 2-1 2 1 2-1 2 1 2-1 2 1V2l-2 1-2-1-2 1-2-1-2 1-2-1-2 1Z"/><path d="M8 8h8"/><path d="M8 12h8"/>"#,
        "sun" => r#"<circle cx="12" cy="12" r="4"/><path d="M12 2v2M12 20v2M4.93 4.93l1.41 1.41M17.66 17.66l1.41 1.41M2 12h2M20 12h2M6.34 17.66l-1.41 1.41M19.07 4.93l-1.41 1.41"/>"#,
        "moon" => r#"<path d="M12 3a6 6 0 0 0 9 9 9 9 0 1 1-9-9Z"/>"#,
        "check-circle" => r#"<circle cx="12" cy="12" r="9"/><path d="m8.5 12.5 2.5 2.5 4.5-5"/>"#,
        "triangle-alert" => r#"<path d="M12 3 22 20H2Z"/><path d="M12 10v4"/><path d="M12 17v.01"/>"#,
        "octagon-alert" => r#"<path d="M7.86 2h8.28L22 7.86v8.28L16.14 22H7.86L2 16.14V7.86Z"/><path d="M12 8v4"/><path d="M12 16v.01"/>"#,
        "arrow-up-right" => r#"<path d="M7 17 17 7"/><path d="M8 7h9v9"/>"#,
        "arrow-down-right" => r#"<path d="M7 7l10 10"/><path d="M17 8v9H8"/>"#,
        "arrow-left" => r#"<path d="M19 12H5"/><path d="m12 19-7-7 7-7"/>"#,
        "chevron-up" => r#"<path d="m6 15 6-6 6 6"/>"#,
        "chevron-down" => r#"<path d="m6 9 6 6 6-6"/>"#,
        "chevrons" => r#"<path d="m7 9 5-5 5 5"/><path d="m7 15 5 5 5-5"/>"#,
        "copy" => r#"<rect x="9" y="9" width="11" height="11" rx="2"/><path d="M5 15V6a2 2 0 0 1 2-2h9"/>"#,
        "check" => r#"<path d="m5 12.5 4.5 4.5L19 7"/>"#,
        "info" => r#"<circle cx="12" cy="12" r="9"/><path d="M12 11v5"/><path d="M12 8v.01"/>"#,
        "user" => r#"<circle cx="12" cy="8" r="4"/><path d="M4 21a8 8 0 0 1 16 0"/>"#,
        "users" => r#"<circle cx="9" cy="8" r="3.5"/><path d="M2.5 20a6.5 6.5 0 0 1 13 0"/><path d="M16 4.5a3.5 3.5 0 0 1 0 7"/><path d="M18 14a6.5 6.5 0 0 1 3.5 6"/>"#,
        "shield" => r#"<path d="M12 3 4 6v6c0 4.5 3.2 8 8 9 4.8-1 8-4.5 8-9V6Z"/><path d="m9 12 2 2 4-4"/>"#,
        "activity" => r#"<path d="M3 12h4l3-8 4 16 3-8h4"/>"#,
        "log-out" => r#"<path d="M9 21H5a2 2 0 0 1-2-2V5a2 2 0 0 1 2-2h4"/><path d="m16 17 5-5-5-5"/><path d="M21 12H9"/>"#,
        "globe" => r#"<circle cx="12" cy="12" r="9"/><path d="M3 12h18"/><path d="M12 3a14 14 0 0 1 0 18 14 14 0 0 1 0-18"/>"#,
        "plus" => r#"<path d="M12 5v14"/><path d="M5 12h14"/>"#,
        "edit" => r#"<path d="M4 20h4L19 9l-4-4L4 16Z"/><path d="m14 6 4 4"/>"#,
        "trash" => r#"<path d="M4 7h16"/><path d="M9 7V4h6v3"/><path d="M6 7l1 13h10l1-13"/>"#,
        "key" => r#"<circle cx="8" cy="15" r="4"/><path d="m11 12 9-9"/><path d="m16 7 3 3"/>"#,
        "lock" => r#"<rect x="5" y="11" width="14" height="10" rx="2"/><path d="M8 11V8a4 4 0 0 1 8 0v3"/>"#,
        "download" => r#"<path d="M12 3v12"/><path d="m7 10 5 5 5-5"/><path d="M5 21h14"/>"#,
        "star" => r#"<path d="m12 3 2.7 5.6 6.1.9-4.4 4.3 1 6.1L12 17l-5.4 2.9 1-6.1L3.2 9.5l6.1-.9Z"/>"#,
        "columns" => r#"<rect x="3" y="4" width="7" height="16" rx="1.5"/><rect x="14" y="4" width="7" height="16" rx="1.5"/>"#,
        "clock" => r#"<circle cx="12" cy="12" r="9"/><path d="M12 7v5l3 2"/>"#,
        "refresh" => r#"<path d="M20 11a8 8 0 1 0-2.3 5.7"/><path d="M20 4v7h-7"/>"#,
        "x" => r#"<path d="M6 6l12 12"/><path d="M18 6 6 18"/>"#,
        _ => "",
    };
    html! {
        svg class=(class) viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75"
            stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false" {
            (PreEscaped(paths))
        }
    }
}

pub fn sr(text: &str) -> Markup {
    html! { span.sr-only { (text) } }
}

/// Término con explicación: botón con subrayado punteado; el tooltip sale con hover, foco o toque.
pub fn term(label: &str, tip: &str) -> Markup {
    html! { button.term type="button" data-tip=(tip) { (label) } }
}

/// Botón "i" junto a un título, con zona táctil ampliada.
pub fn info_btn(label: &str, tip: &str) -> Markup {
    html! { button.info type="button" data-tip=(tip) aria-label=(label) { (icon_sized("info", "icon-sm")) } }
}

// ───────────────────────── Layout ─────────────────────────

struct NavItem {
    href: &'static str,
    label: &'static str,
    short: &'static str,
    icon: &'static str,
    badge: Option<&'static str>,
}

const NAV_COMPANIES: [NavItem; 4] = [
    NavItem { href: "/sok", label: "nav.search", short: "nav.search.short", icon: "search", badge: None },
    NavItem { href: "/bevakning", label: "nav.watch", short: "nav.watch.short", icon: "star", badge: None },
    NavItem { href: "/comparar", label: "nav.compare", short: "nav.compare.short", icon: "columns", badge: None },
    NavItem { href: "/historial", label: "nav.history", short: "nav.history.short", icon: "clock", badge: None },
];
const NAV_BUSINESS: [NavItem; 3] = [
    NavItem { href: "/likviditet", label: "nav.liquidity", short: "nav.liquidity.short", icon: "trend", badge: None },
    NavItem { href: "/sie", label: "nav.sie", short: "nav.sie.short", icon: "upload", badge: None },
    NavItem { href: "/fakturor", label: "nav.invoices", short: "nav.invoices.short", icon: "receipt", badge: None },
];
const NAV_USERS: NavItem = NavItem { href: "/users", label: "nav.users", short: "nav.users", icon: "users", badge: None };
const NAV_ACTIVITY: NavItem = NavItem { href: "/activity", label: "nav.activity", short: "nav.activity", icon: "activity", badge: None };

const FONTS_URL: &str = "https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:wght@500;700&family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans:wght@400;500;600&display=swap";
const FAVICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='8' fill='%230b6e75'/%3E%3Cpath d='M21 11.5c-1-1.6-2.8-2.5-5-2.5-2.9 0-4.8 1.5-4.8 3.6 0 5 10 2.7 10 7.6 0 2.3-2.2 3.8-5.2 3.8-2.4 0-4.4-1-5.4-2.7' fill='none' stroke='white' stroke-width='2.6' stroke-linecap='round'/%3E%3C/svg%3E";

/// Se ejecuta antes de pintar: aplica el tema guardado (sin parpadeo) y marca que hay JavaScript.
const HEAD_SCRIPT: &str = r#"(function(){var r=document.documentElement;r.classList.add('js');try{var t=localStorage.getItem('theme');if(t==='light'||t==='dark')r.dataset.theme=t;}catch(e){}})();"#;

const THEME_SCRIPT: &str = r#"(function(){var b=document.getElementById('theme-toggle');if(!b)return;var r=document.documentElement;
function cur(){return r.dataset.theme||(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light');}
function sync(){b.setAttribute('aria-pressed',cur()==='dark'?'true':'false');}
sync();
b.addEventListener('click',function(){var n=cur()==='dark'?'light':'dark';r.dataset.theme=n;try{localStorage.setItem('theme',n);}catch(e){}sync();});
matchMedia('(prefers-color-scheme: dark)').addEventListener('change',sync);})();"#;

/// Tooltips (`[data-tip]`), copiar al portapapeles (`[data-copy]`), atajo "/" hacia el buscador, menús
/// desplegables (se cierran al hacer clic fuera o con Escape) y horas en la zona horaria del navegador.
/// Los textos que muestra el script vienen de atributos `data-*` del `<body>` (ya traducidos).
const UI_SCRIPT: &str = r#"(function(){
var body=document.body,D=function(k,f){return body.getAttribute('data-'+k)||f;};
var tip=document.createElement('div');tip.id='tip';tip.setAttribute('role','tooltip');tip.hidden=true;document.body.appendChild(tip);
var cur=null,canHover=matchMedia('(hover: hover)'),timer=null;
function place(){if(!cur)return;var r=cur.getBoundingClientRect(),w=tip.offsetWidth,h=tip.offsetHeight,m=8;
var x=Math.max(m,Math.min(r.left+r.width/2-w/2,innerWidth-w-m));var y=r.top-h-m;if(y<m)y=r.bottom+m;
tip.style.left=x+'px';tip.style.top=y+'px';}
function show(el){var t=el.getAttribute('data-tip');if(!t)return;if(cur&&cur!==el)cur.removeAttribute('aria-describedby');
cur=el;tip.textContent=t;tip.hidden=false;el.setAttribute('aria-describedby','tip');place();}
function hide(){if(cur){cur.removeAttribute('aria-describedby');cur=null;}tip.hidden=true;}
function tgt(e){return e.target.closest?e.target.closest('[data-tip]'):null;}
function closeMenus(except){document.querySelectorAll('details.menu[open]').forEach(function(d){if(d!==except)d.removeAttribute('open');});}
document.addEventListener('mouseover',function(e){var el=tgt(e);if(el&&canHover.matches)show(el);});
document.addEventListener('mouseout',function(e){var el=tgt(e);if(el&&(!e.relatedTarget||!el.contains(e.relatedTarget)))hide();});
document.addEventListener('focusin',function(e){var el=tgt(e);if(el&&el.matches(':focus-visible'))show(el);});
document.addEventListener('focusout',function(e){if(tgt(e))hide();});
document.addEventListener('keydown',function(e){
if(e.key==='Escape'){hide();closeMenus(null);}
if(e.key==='/'&&!e.ctrlKey&&!e.metaKey&&!e.altKey){var q=document.getElementById('q'),a=document.activeElement;
if(q&&a!==q&&!(a&&/^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)||a&&a.isContentEditable)){e.preventDefault();q.focus();q.select();}}});
document.addEventListener('click',function(e){
var m=e.target.closest?e.target.closest('details.menu'):null;closeMenus(m);
var c=e.target.closest?e.target.closest('[data-copy]'):null;
if(c){var v=c.getAttribute('data-copy'),orig=c.getAttribute('data-tip'),live=document.getElementById('live');
var done=function(ok){c.classList.toggle('copied',ok);c.setAttribute('data-tip',ok?D('copied','Copied'):D('copy-failed','Could not copy'));if(live)live.textContent=ok?D('copied','Copied')+': '+v:D('copy-failed','Could not copy');show(c);
clearTimeout(timer);timer=setTimeout(function(){c.classList.remove('copied');c.setAttribute('data-tip',orig);if(cur===c)show(c);},1800);};
var legacy=function(){var t=document.createElement('textarea');t.value=v;t.setAttribute('readonly','');t.style.cssText='position:fixed;top:0;left:0;opacity:0';document.body.appendChild(t);t.select();var ok=false;try{ok=document.execCommand('copy');}catch(x){}document.body.removeChild(t);return ok;};
if(navigator.clipboard&&navigator.clipboard.writeText){navigator.clipboard.writeText(v).then(function(){done(true);},function(){done(legacy());});}else{done(legacy());}return;}
var el=tgt(e);
if(el&&el.tagName!=='A'&&!canHover.matches){(cur===el&&!tip.hidden)?hide():show(el);}else if(!el){hide();}});
addEventListener('scroll',hide,true);addEventListener('resize',hide);
document.querySelectorAll('time[datetime]').forEach(function(t){var d=new Date(t.getAttribute('datetime'));if(isNaN(d))return;
try{t.textContent=d.toLocaleString(document.documentElement.lang||undefined,{year:'numeric',month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit',second:'2-digit'});t.title=t.getAttribute('datetime');}catch(e){}});
})();"#;

fn brand_mark() -> Markup {
    html! {
        svg.brand-mark viewBox="0 0 32 32" aria-hidden="true" focusable="false" {
            defs {
                linearGradient #bm-g x1="0" y1="0" x2="1" y2="1" {
                    stop offset="0" stop-color="#2fb3ba" {}
                    stop offset="1" stop-color="#0b6e75" {}
                }
            }
            rect width="32" height="32" rx="9" fill="url(#bm-g)" {}
            path d="M21 11.5c-1-1.6-2.8-2.5-5-2.5-2.9 0-4.8 1.5-4.8 3.6 0 5 10 2.7 10 7.6 0 2.3-2.2 3.8-5.2 3.8-2.4 0-4.4-1-5.4-2.7"
                fill="none" stroke="#fff" stroke-width="2.6" stroke-linecap="round" {}
        }
    }
}

pub fn brand_mark_pub() -> Markup {
    brand_mark()
}

fn head(c: &Ctx, title: &str) -> Markup {
    html! {
        head {
            meta charset="utf-8";
            meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
            meta name="color-scheme" content="light dark";
            meta name="theme-color" content="#dcebee" media="(prefers-color-scheme: light)";
            meta name="theme-color" content="#0b1a21" media="(prefers-color-scheme: dark)";
            meta name="referrer" content="same-origin";
            title { (title) " — Siffra" }
            meta name="description" content=(c.t("app.description"));
            link rel="icon" href=(FAVICON);
            link rel="preconnect" href="https://fonts.googleapis.com";
            link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
            link rel="stylesheet" href=(FONTS_URL);
            link rel="stylesheet" href=(format!("/static/styles.css?v={}", crate::handlers::css_version()));
            script { (PreEscaped(HEAD_SCRIPT)) }
        }
    }
}

fn body_attrs_script(c: &Ctx) -> Markup {
    // El script lee los textos traducidos de atributos del <body>; se fijan con un pequeño script para no
    // tener que repetirlos en cada plantilla.
    let js = format!(
        "document.body.setAttribute('data-copied',{});document.body.setAttribute('data-copy-failed',{});",
        js_string(c.t("ui.copied")),
        js_string(c.t("ui.copy_failed"))
    );
    html! { script { (PreEscaped(js)) } }
}

fn js_string(s: &str) -> String {
    format!("\"{}\"", s.replace('\\', "\\\\").replace('"', "\\\"").replace('<', "\\u003c").replace('\n', " "))
}

/// Documento completo con navegación. `title` ya viene traducido.
pub fn layout(c: &Ctx, title: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang=(c.lang.html_lang()) {
            (head(c, title))
            body {
                a.skip-link href="#main" { (c.t("ui.skip")) }
                div.backdrop aria-hidden="true" { span.blob.b1 {} span.blob.b2 {} span.blob.b3 {} }
                div.shell {
                    (sidebar(c))
                    main #main.main tabindex="-1" { div.page { (content) } }
                }
                div #live.sr-only role="status" aria-live="polite" {}
                (body_attrs_script(c))
                script { (PreEscaped(THEME_SCRIPT)) }
                script { (PreEscaped(UI_SCRIPT)) }
            }
        }
    }
}

/// Pantallas sin sesión (inicio de sesión): tarjeta centrada de vidrio sobre el fondo.
pub fn auth_layout(c: &Ctx, title: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang=(c.lang.html_lang()) {
            (head(c, title))
            body.auth-body {
                div.backdrop aria-hidden="true" { span.blob.b1 {} span.blob.b2 {} span.blob.b3 {} }
                main #main.auth-main tabindex="-1" {
                    div.auth-top { (lang_menu(c)) (theme_button(c)) }
                    (content)
                }
                div #live.sr-only role="status" aria-live="polite" {}
                (body_attrs_script(c))
                script { (PreEscaped(THEME_SCRIPT)) }
                script { (PreEscaped(UI_SCRIPT)) }
            }
        }
    }
}

fn theme_button(c: &Ctx) -> Markup {
    html! {
        button #theme-toggle.icon-btn type="button" aria-pressed="false" aria-label=(c.t("ui.dark_mode")) data-tip=(c.t("ui.theme_tip")) {
            span.icon-moon { (icon("moon")) }
            span.icon-sun { (icon("sun")) }
        }
    }
}

fn lang_menu(c: &Ctx) -> Markup {
    html! {
        details.menu.lang-menu {
            summary.menu-btn aria-label=(c.t("ui.language")) {
                (icon("globe")) span.menu-label { (c.lang.short()) }
            }
            div.menu-pop {
                ul.menu-list {
                    @for l in Lang::ALL {
                        li {
                            a.menu-item href=(c.url_for_lang(l)) lang=(l.code()) hreflang=(l.code()) aria-current=[(l == c.lang).then_some("true")] {
                                span.menu-code { (l.short()) } (l.name())
                            }
                        }
                    }
                }
            }
        }
    }
}

fn user_menu(c: &Ctx, user: &User) -> Markup {
    html! {
        details.menu.user-menu {
            summary.menu-btn aria-label=(c.tf("ui.account_menu", &[&user.name])) {
                span.avatar { (user.initials()) }
                span.menu-label.user-name { (user.name) }
            }
            div.menu-pop {
                div.menu-head {
                    span.avatar.big { (user.initials()) }
                    div.menu-who { strong { (user.name) } span.muted { (user.label()) } }
                }
                (role_pill(c, user.role))
                ul.menu-list {
                    li { a.menu-item href="/profile" { (icon_sized("user", "icon-sm")) (c.t("nav.profile")) } }
                    @if crate::auth::can_admin_users(user) {
                        li { a.menu-item href="/users" { (icon_sized("users", "icon-sm")) (c.t("nav.users")) } }
                    }
                    @if crate::auth::can_view_activity(user) {
                        li { a.menu-item href="/activity" { (icon_sized("activity", "icon-sm")) (c.t("nav.activity")) } }
                    }
                    li {
                        form method="post" action="/logout" {
                            (c.csrf_input())
                            button.menu-item.danger type="submit" { (icon_sized("log-out", "icon-sm")) (c.t("nav.logout")) }
                        }
                    }
                }
            }
        }
    }
}

pub fn role_pill(c: &Ctx, role: Role) -> Markup {
    let class = match role {
        Role::Superadmin => "pill pill-super",
        Role::Admin => "pill pill-admin",
        Role::User => "pill pill-user",
    };
    html! { span class=(class) { (c.t(role.label_key())) } }
}

/// `/foretag/<org>` se considera parte de "Buscar empresas".
fn nav_active(pathname: &str, href: &str) -> bool {
    pathname == href
        || pathname.starts_with(&format!("{href}/"))
        || (href == "/sok" && pathname.starts_with("/foretag"))
}

fn nav_link(c: &Ctx, item: &NavItem) -> Markup {
    let active = nav_active(&c.path, item.href);
    html! {
        a.nav-link href=(item.href) aria-current=[active.then_some("page")] {
            (icon(item.icon))
            span.nav-text {
                span.label-long { (c.t(item.label)) }
                span.label-short { (c.t(item.short)) }
                @if let Some(badge) = item.badge {
                    span.nav-badge { (badge) (sr(c.t("nav.new"))) }
                }
            }
        }
    }
}

fn sidebar(c: &Ctx) -> Markup {
    let admin = c.user.as_ref().is_some_and(crate::auth::can_admin_users);
    let super_ = c.user.as_ref().is_some_and(crate::auth::can_view_activity);
    html! {
        aside.sidebar {
            div.side-head {
                a.brand href="/sok" aria-label=(c.t("ui.brand_label")) { (brand_mark()) span.brand-word { "Sif" span.accent { "f" } "ra" } }
                div.side-actions {
                    (lang_menu(c))
                    (theme_button(c))
                    @if let Some(u) = &c.user { (user_menu(c, u)) }
                }
            }
            nav.side-nav aria-label=(c.t("ui.main_menu")) {
                div.nav-group {
                    div.nav-heading { (c.t("nav.group.companies")) }
                    // Liquidez, SIE y facturas son maquetas con datos de EJEMPLO: solo en modo demo.
                    @for item in NAV_COMPANIES.iter() { (nav_link(c, item)) }
                }
                @if c.demo {
                    div.nav-group {
                        div.nav-heading { (c.t("nav.group.business")) }
                        @for item in NAV_BUSINESS.iter() { (nav_link(c, item)) }
                    }
                }
                @if admin {
                    div.nav-group.nav-admin {
                        div.nav-heading { (c.t("nav.group.admin")) }
                        (nav_link(c, &NAV_USERS))
                        @if super_ { (nav_link(c, &NAV_ACTIVITY)) }
                    }
                }
            }
        }
    }
}

// ───────────────────────── Componentes ─────────────────────────

fn sev_class(s: Severity) -> &'static str {
    match s {
        Severity::Good => "good",
        Severity::Warn => "warn",
        Severity::Bad => "bad",
    }
}

pub fn example_badge(c: &Ctx) -> Markup {
    html! { span.example-badge data-tip=(c.t("common.example_tip")) { (c.t("common.example")) } }
}

fn risk_pill(c: &Ctx, level: RiskLevel) -> Markup {
    let key = match level {
        Severity::Good => "risk.low",
        Severity::Warn => "risk.watch",
        Severity::Bad => "risk.high",
    };
    html! { span class={"pill pill-" (sev_class(level))} { span.pill-dot aria-hidden="true" {} (c.t(key)) } }
}

fn status_pill(text: &str) -> Markup {
    html! { span.pill.pill-good { span.pill-dot aria-hidden="true" {} (text) } }
}

/// Una señal con icono propio por gravedad y texto para lectores de pantalla (no depende solo del color).
fn alert_row(c: &Ctx, severity: Severity, body: Markup) -> Markup {
    let (icon_name, label_key) = match severity {
        Severity::Good => ("check-circle", "sev.good"),
        Severity::Warn => ("triangle-alert", "sev.warn"),
        Severity::Bad => ("octagon-alert", "sev.bad"),
    };
    html! {
        li {
            span class={"alert-icon sev-" (sev_class(severity))} { (icon_sized(icon_name, "icon")) }
            span { (sr(c.t(label_key))) (body) }
        }
    }
}

/// Millones de coronas con un decimal, para las etiquetas de las barras.
fn mkr(c: &Ctx, tkr: f64) -> String {
    c.dec(tkr / 1000.0, 1)
}

fn benchmark_row(c: &Ctx, label_key: &str, tip_key: &str, pair: BenchmarkPair, scale_max: f64) -> Markup {
    let label = c.t(label_key);
    let value_pct = (pair.value / scale_max * 100.0).clamp(0.0, 100.0);
    let median_pct = (pair.median / scale_max * 100.0).clamp(0.0, 100.0);
    let (v, m) = (c.pct(pair.value), c.pct(pair.median));
    let aria = c.tf("bench.aria", &[label, &v, &m]);
    let diff = pair.value - pair.median;
    let tip = c.tf(
        "bench.tip",
        &[label, &v, &m, c.t(if diff >= 0.0 { "bench.above" } else { "bench.below" }), &c.dec(diff.abs(), 1)],
    );
    html! {
        div.bench {
            div.bench-top {
                span.bench-label { (term(label, c.t(tip_key))) }
                span.bench-val {
                    span.neg[pair.value < 0.0] { (v) }
                    span.bench-median-text { (c.tf("bench.median_text", &[&m])) }
                }
            }
            div.bench-track role="img" tabindex="0" aria-label=(aria) data-tip=(tip) {
                div.bench-fill style=(format!("width:{}%", value_pct)) {}
                span.bench-marker style=(format!("left:{}%", median_pct)) {}
            }
        }
    }
}

fn bench_legend(c: &Ctx) -> Markup {
    html! {
        div.legend aria-hidden="true" {
            span.legend-item { span.swatch.fill {} (c.t("bench.legend.company")) }
            span.legend-item { span.swatch.marker {} (c.t("bench.legend.median")) }
        }
    }
}

/// Barras de comparación + leyenda + nota de fuente. Se sirve en la ficha o como fragmento diferido.
/// El valor de la empresa es de EJEMPLO; la mediana es real si SCB respondió.
pub fn benchmark_fragment(c: &Ctx, company: &Company, medians: Option<&SectorMedians>, notice: Option<&str>) -> Markup {
    let b = &company.benchmarks;
    let with_median = |pair: BenchmarkPair, real: Option<f64>| BenchmarkPair { median: real.unwrap_or(pair.median), ..pair };
    let margin = with_median(b.margin, medians.map(|m| m.margin));
    let solidity = with_median(b.solidity, medians.map(|m| m.solidity));
    let liquidity = with_median(b.liquidity, medians.map(|m| m.liquidity));
    html! {
        (benchmark_row(c, "term.margin", "tip.margin", margin, 30.0))
        (benchmark_row(c, "term.solidity", "tip.solidity.bench", solidity, 80.0))
        (benchmark_row(c, "term.liquidity", "tip.liquidity", liquidity, 250.0))
        (bench_legend(c))
        @if let Some(n) = notice {
            p.notice role="status" { (n) }
        }
        @if let Some(m) = medians {
            @let label = if m.sni_label.is_empty() { String::new() } else { format!(" ({})", m.sni_label) };
            @let size = if m.size_class == "TOT" { c.t("bench.all_sizes").to_string() } else { c.tf("bench.size", &[&m.size_class.replace("001", "0")]) };
            p.note {
                (c.tf("bench.note.real", &[m.sni_code.as_str(), &label, &size, m.year.as_str()]))
                @if !m.exact_sni || !m.exact_size {
                    " " (c.tf("bench.note.fallback", &[company.sni_code, company.employee_range]))
                }
                " " (c.t("bench.note.own")) " " (example_badge(c)) "."
            }
        } @else {
            p.note { (c.tf("bench.note.example", &[company.sni_code])) " " (example_badge(c)) }
        }
    }
}

/// Paso "redondo" (1, 2, 2.5, 5 × 10^k) mayor o igual que `x`, para que el eje salga con cifras limpias.
fn nice_step(x: f64) -> f64 {
    if x <= 0.0 {
        return 1.0;
    }
    let pow = 10f64.powf(x.log10().floor());
    [1.0, 2.0, 2.5, 5.0, 10.0].iter().map(|m| m * pow).find(|s| *s >= x * 0.999_999).unwrap_or(10.0 * pow)
}

/// Gráfico de barras SVG de la omsättning de 5 años (datos de ejemplo).
fn revenue_chart(c: &Ctx, revenue: &FiveYearSeries) -> Markup {
    bar_chart(c, &FINANCIAL_YEARS, revenue)
}

/// Gráfico de barras SVG de la facturación con etiqueta de valor en cada barra, tooltips y eje adaptable
/// (millones: mkr; cifras pequeñas: tkr). `labels` y `values` tienen la misma longitud.
fn bar_chart(c: &Ctx, labels: &[&str], values: &[i64]) -> Markup {
    let n = values.len().min(labels.len());
    if n == 0 {
        return html! {};
    }
    let (labels, values) = (&labels[..n], &values[..n]);
    let (w, h) = (360.0_f64, 230.0_f64);
    let (pl, pb, pt, pr) = (58.0_f64, 28.0_f64, 22.0_f64, 6.0_f64);
    let max = values.iter().copied().max().unwrap_or(0).max(0) as f64;
    let top = 4.0 * nice_step((max / 4.0).max(1.0));
    let in_mkr = top >= 5000.0;
    let (unit_tkr, unit_mkr) = (c.t("unit.tkr"), c.t("unit.mkr"));
    let axis_label = |v: f64| if in_mkr { format!("{} {unit_mkr}", c.num(v / 1000.0)) } else { format!("{} {unit_tkr}", c.num(v)) };
    let bar_label = |v: i64| if in_mkr { mkr(c, v as f64) } else { c.int(v) };
    let bw = (w - pl - pr) / n as f64;

    let grid_lines: Vec<(f64, f64)> = (0..5)
        .map(|i| {
            let value = top / 4.0 * i as f64;
            (value, h - pb - (h - pb - pt) * value / top)
        })
        .collect();
    let title = c.tf("chart.rev.title", &[unit_tkr]);
    let desc = labels.iter().zip(values).map(|(l, v)| format!("{l}: {} {unit_tkr}", c.int(*v))).collect::<Vec<_>>().join(", ");

    html! {
        svg.chart viewBox=(format!("0 0 {} {}", w, h)) role="group" aria-labelledby="rc-title rc-desc" {
            title #rc-title { (title) }
            desc #rc-desc { (desc) }
            @for (value, y) in grid_lines.iter() {
                g {
                    line x1=(pl) x2=(w - pr) y1=(y) y2=(y) stroke="var(--line)" stroke-width="1" {}
                    text x=(pl - 8.0) y=(y + 4.0) text-anchor="end" { (axis_label(*value)) }
                }
            }
            @for i in 0..n {
                @let v = values[i];
                @let vf = v.max(0) as f64;
                @let bar_h = (h - pb - pt) * vf / top;
                @let x = pl + i as f64 * bw + bw * 0.18;
                @let bar_w = bw * 0.64;
                @let y = h - pb - bar_h;
                @let is_last = i == n - 1;
                @let tip = if i > 0 && values[i - 1] > 0 {
                    let ch = (v as f64 / values[i - 1] as f64 - 1.0) * 100.0;
                    c.tf("chart.rev.tip_change", &[labels[i], &c.int(v), unit_tkr, if ch >= 0.0 { "+" } else { "−" }, &c.dec(ch.abs(), 1), labels[i - 1]])
                } else {
                    c.tf("chart.rev.tip", &[labels[i], &c.int(v), unit_tkr])
                };
                g.bar-group tabindex="0" role="img" aria-label=(tip) data-tip=(tip) {
                    rect.hit x=(pl + i as f64 * bw) y=(pt) width=(bw) height=(h - pb - pt) fill="transparent" {}
                    rect.bar x=(x) y=(y) width=(bar_w) height=(bar_h) rx="3"
                        fill=(if is_last { "url(#bar-grad)" } else { "var(--bar2)" }) {}
                    text x=(x + bar_w / 2.0) y=(h - 9.0) text-anchor="middle" { (labels[i]) }
                    text.value-label.strong[is_last] x=(x + bar_w / 2.0) y=(y - 6.0) text-anchor="middle" { (bar_label(v)) }
                }
            }
            defs {
                linearGradient #bar-grad x1="0" y1="0" x2="0" y2="1" {
                    stop offset="0" stop-color="var(--bar-top)" {}
                    stop offset="1" stop-color="var(--bar)" {}
                }
            }
        }
    }
}

/// Gráfico de barras SVG de la caja proyectada, a partir del saldo inicial y las entradas/salidas semanales.
fn cash_flow_chart(c: &Ctx, weeks: &[CashWeek], start_balance: i64) -> Markup {
    const THRESHOLD: i64 = 150; // por debajo de este saldo la barra se marca como riesgo
    let unit = c.t("unit.tkr");
    let mut balance = start_balance;
    let points: Vec<i64> = weeks
        .iter()
        .map(|wk| {
            balance += wk.inflow - wk.outflow;
            balance
        })
        .collect();
    let min = *points.iter().min().unwrap_or(&0);
    let max = *points.iter().max().unwrap_or(&0);

    let (w, h) = (520.0_f64, 240.0_f64);
    let (pl, pb, pt) = (52.0_f64, 28.0_f64, 26.0_f64);
    let lo = (min.min(0)) as f64;
    let hi = (max as f64 / 100.0).ceil() * 100.0;
    let bw = (w - pl - 8.0) / weeks.len() as f64;

    let y = |v: f64| h - pb - (h - pb - pt) * (v - lo) / (hi - lo);
    let grid_values = [lo, js_round((lo + hi) / 2.0 / 100.0) * 100.0, hi];
    let lowest_idx = points.iter().position(|&p| p == min).unwrap_or(0);
    let week_label = |n: u32| c.tf("chart.week_short", &[&n.to_string()]);
    let desc = weeks.iter().zip(&points).map(|(wk, p)| format!("{}: {} {unit}", c.tf("chart.week_n", &[&wk.week.to_string()]), c.int(*p))).collect::<Vec<_>>().join(", ");
    let threshold_text = c.tf("chart.cash.limit_label", &[&THRESHOLD.to_string()]);

    html! {
        figure {
            div.chart-scroll {
                svg.chart viewBox=(format!("0 0 {} {}", w, h)) role="group" aria-labelledby="cc-title cc-desc" {
                    title #cc-title { (c.tf("chart.cash.title", &[unit])) }
                    desc #cc-desc { (c.t("chart.cash.desc")) " " (desc) }
                    defs {
                        pattern #hatch width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)" {
                            rect width="6" height="6" fill="var(--bad)" {}
                            rect width="2" height="6" fill="var(--surface)" {}
                        }
                        linearGradient #bar-grad2 x1="0" y1="0" x2="0" y2="1" {
                            stop offset="0" stop-color="var(--bar-top)" {}
                            stop offset="1" stop-color="var(--bar)" {}
                        }
                    }
                    text x="0" y="12" { (unit) }
                    @for v in grid_values.iter() {
                        g {
                            line x1=(pl) x2=(w - 8.0) y1=(y(*v)) y2=(y(*v)) stroke="var(--line)" {}
                            text x=(pl - 8.0) y=(y(*v) + 4.0) text-anchor="end" { (c.num(*v)) }
                        }
                    }
                    @for (i, v) in points.iter().enumerate() {
                        @let v = *v;
                        @let x = pl + i as f64 * bw + bw * 0.15;
                        @let bar_w = bw * 0.7;
                        @let y0 = y(0.0);
                        @let y1 = y(v as f64);
                        @let net = weeks[i].inflow - weeks[i].outflow;
                        @let tip = c.tf("chart.cash.bar_tip", &[&weeks[i].week.to_string(), &c.int(v), &c.int(weeks[i].inflow), &c.int(weeks[i].outflow), &c.int(net), unit])
                            + if v < THRESHOLD { c.t("chart.cash.below_suffix") } else { "" };
                        g.bar-group tabindex="0" role="img" aria-label=(tip) data-tip=(tip) {
                            rect.hit x=(pl + i as f64 * bw) y=(pt) width=(bw) height=(h - pb - pt) fill="transparent" {}
                            rect.bar x=(x) y=(y0.min(y1)) width=(bar_w) height=((y1 - y0).abs()) rx="3"
                                fill=(if v < THRESHOLD { "url(#hatch)" } else { "url(#bar-grad2)" })
                                stroke=(if v < THRESHOLD { "var(--bad)" } else { "none" }) {}
                            text x=(x + bar_w / 2.0) y=(h - 9.0) text-anchor="middle" { (week_label(weeks[i].week)) }
                            @if i == lowest_idx {
                                text.value-label.strong x=(x + bar_w / 2.0) y=(y1 - 6.0) text-anchor="middle" { (c.int(v)) }
                            }
                        }
                    }
                    g.threshold-group tabindex="0" role="img" aria-label=(c.tf("chart.cash.limit_aria", &[&THRESHOLD.to_string(), unit]))
                        data-tip=(c.tf("chart.cash.limit_tip", &[&THRESHOLD.to_string(), unit])) {
                        rect x=(w - 110.0) y=(y(THRESHOLD as f64) - 20.0) width="102" height="20" fill="transparent" {}
                        line.threshold x1=(pl) x2=(w - 8.0) y1=(y(THRESHOLD as f64)) y2=(y(THRESHOLD as f64)) {}
                        text.threshold-label x=(w - 10.0) y=(y(THRESHOLD as f64) - 5.0) text-anchor="end" { (threshold_text) }
                    }
                }
            }
            p.scroll-hint { (c.t("chart.scroll_hint")) }
            div.legend {
                span.legend-item { span.swatch.fill {} (c.t("chart.cash.legend.cash")) }
                span.legend-item { span.swatch.hatch {} (c.t("chart.cash.legend.below")) }
                span.legend-item { span.swatch.dash {} (c.tf("chart.cash.limit_legend", &[&THRESHOLD.to_string(), unit])) }
            }
            figcaption.chart-caption {
                (c.t("chart.cash.caption_a")) " " strong { (c.int(min)) " " (unit) } " " (c.tf("chart.cash.caption_b", &[&weeks[lowest_idx].week.to_string()]))
            }
        }
        details.chart-data {
            summary { (c.t("chart.show_table")) }
            div.table-wrap {
                table {
                    caption.sr-only { (c.tf("chart.cash.table_caption", &[unit])) }
                    thead { tr {
                        th scope="col" { (c.t("chart.cash.col.week")) }
                        th.right scope="col" { (c.t("chart.cash.col.in")) }
                        th.right scope="col" { (c.t("chart.cash.col.out")) }
                        th.right scope="col" { (c.t("chart.cash.col.cash")) }
                    } }
                    tbody {
                        @for (wk, p) in weeks.iter().zip(&points) {
                            tr {
                                th scope="row" { (wk.week) }
                                td.right.num { (c.int(wk.inflow)) }
                                td.right.num { (c.int(wk.outflow)) }
                                td.right.num { (c.int(*p)) }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ───────────────────────── Búsqueda ─────────────────────────

fn sok_url(q: &str, sort: Option<(&str, &str)>) -> String {
    let mut url = String::from("/sok");
    let mut sep = '?';
    if !q.is_empty() {
        url.push_str(&format!("{sep}q={}", urlencode(q)));
        sep = '&';
    }
    if let Some((col, dir)) = sort {
        url.push_str(&format!("{sep}sort={col}&dir={dir}"));
    }
    url
}

/// Dirección por defecto al ordenar por primera vez: texto ascendente, cifras descendentes.
fn default_dir(col: &str) -> &'static str {
    if col == "name" { "asc" } else { "desc" }
}

#[allow(clippy::too_many_arguments)]
fn sortable_th(c: &Ctx, label: &str, col: &str, q: &str, sort: Option<&str>, dir: &str, right: bool, hide_sm: bool) -> Markup {
    let current = (sort == Some(col)).then_some(dir);
    let next = match current {
        Some("asc") => "desc",
        Some(_) => "asc",
        None => default_dir(col),
    };
    let aria_sort = match current {
        Some("asc") => Some("ascending"),
        Some(_) => Some("descending"),
        None => None,
    };
    let icon_name = match current {
        Some("asc") => "chevron-up",
        Some(_) => "chevron-down",
        None => "chevrons",
    };
    let what = c.t(match col {
        "name" => "sok.sort.company",
        "revenue" => "sok.sort.revenue",
        _ => "sok.sort.result",
    });
    let tip = c.tf("sok.sort.tip", &[c.t(if next == "asc" { "sok.sort.asc" } else { "sok.sort.desc" }), what]);
    html! {
        th.right[right].hide-sm[hide_sm] scope="col" aria-sort=[aria_sort] {
            a.sort-link href=(sok_url(q, Some((col, next)))) data-tip=(tip) {
                (label) (icon_sized(icon_name, "icon-sm"))
            }
        }
    }
}

/// Búsquedas de ejemplo (empresas reales conocidas) para la portada del buscador y el estado vacío fuera del modo demo.
const REAL_EXAMPLES: [&str; 4] = ["Volvo", "Spotify", "Ericsson", "556703-7485"];

const SEARCH_SCRIPT: &str = r#"(function(){var i=document.getElementById('q');if(!i)return;
if(i.value&&location.search.indexOf('q=')>-1&&!document.querySelector('[aria-sort]')){i.focus();var n=i.value.length;try{i.setSelectionRange(n,n);}catch(e){}}
var t;i.addEventListener('input',function(){clearTimeout(t);t=setTimeout(function(){i.form.submit();},350);});})();"#;

/// Resultados de la búsqueda por nombre en el índice del registro. `more`: hay más de los que se muestran.
pub struct RegistryHits {
    pub hits: Vec<Hit>,
    pub more: bool,
}

/// Nombre de la forma jurídica (`AB-ORGFO` → "Aktiebolag"); si no hay traducción, el código sin sufijo.
fn form_label(c: &Ctx, code: &str) -> String {
    let key = format!("form.{}", code.trim_end_matches("-ORGFO").to_lowercase());
    if i18n::has_key(&key) { c.t(&key).to_string() } else { code.trim_end_matches("-ORGFO").to_string() }
}

/// `5560125790` → `556012-5790`; cualquier otra forma (p. ej. una identidad de 12 dígitos) se deja tal cual.
pub fn format_orgnr(orgnr: &str) -> String {
    if orgnr.len() == 10 && orgnr.bytes().all(|b| b.is_ascii_digit()) { format!("{}-{}", &orgnr[..6], &orgnr[6..]) } else { orgnr.to_string() }
}

fn registry_row(c: &Ctx, h: &Hit) -> Markup {
    // Solo las identidades que son número de organización tienen ficha en vivo; el resto no se puede abrir.
    let linkable = h.id_type == "ORGNR-IDORG" && h.orgnr.len() == 10;
    let city = h.city.clone().unwrap_or_default();
    html! {
        tr.dim[h.dereg.is_some()] {
            td.top {
                @if linkable {
                    a.company-link href=(format!("/foretag/{}", h.orgnr)) { (h.name) }
                } @else {
                    span.company-plain { (h.name) }
                }
                div.org-sub {
                    (format_orgnr(&h.orgnr)) " · " (form_label(c, &h.form))
                    @if let Some(d) = &h.dereg { " · " (c.tf("live.deregistered", &[d])) }
                    span.city-sub { (city) }
                }
            }
            td.top.hide-sm { (city) }
            td.top.right.num.mono { "—" }
            td.top.right.num.mono.hide-sm { "—" }
            td.top { "—" }
        }
    }
}

pub fn sok_page(c: &Ctx, query: &str, sort: Option<&str>, dir: Option<&str>, live: Option<&Organisation>, registry: Option<&RegistryHits>) -> Markup {
    let mut results = if c.demo { search_example_companies(query) } else { Vec::new() };
    // Sin modo demo y sin texto de búsqueda: portada de búsqueda en vez de una lista (no hay lista que mostrar).
    let intro = !c.demo && query.trim().is_empty();
    let live = live.filter(|_| results.is_empty());
    let reg_hits: &[Hit] = registry.map(|r| r.hits.as_slice()).unwrap_or(&[]);
    let sort = sort.filter(|s| ["name", "revenue", "result"].contains(s));
    let dir = match dir {
        Some("asc") => "asc",
        Some("desc") => "desc",
        _ => sort.map(default_dir).unwrap_or("asc"),
    };
    match sort {
        Some("name") => results.sort_by_key(|co| co.name.to_lowercase()),
        Some("revenue") => results.sort_by_key(|co| co.financials.revenue[4]),
        Some("result") => results.sort_by_key(|co| co.financials.result[4]),
        _ => {}
    }
    if sort.is_some() && dir == "desc" {
        results.reverse();
    }

    let total = results.len() + usize::from(live.is_some()) + reg_hits.len();
    let count_text = if query.trim().is_empty() {
        c.tf(if total == 1 { "sok.count.one_company" } else { "sok.count.companies" }, &[&total.to_string()])
    } else {
        c.tf(if total == 1 { "sok.count.hit" } else { "sok.count.hits" }, &[&total.to_string(), query.trim()])
    };
    let unit = c.t("unit.tkr");

    layout(
        c,
        c.t("nav.search"),
        html! {
            div.page-head {
                h1.page-title { (c.t("nav.search")) }
                @if c.demo {
                    p.lead { (c.t("sok.lead")) " " strong { (c.t("common.example_upper")) } " " (c.t("sok.lead_b")) }
                } @else {
                    p.lead { (c.t("sok.lead.real")) }
                }
            }
            form.search role="search" method="get" action="/sok" {
                label.field-label for="q" { (c.t("sok.label")) }
                div.search-row {
                    div.search-field {
                        (icon("search"))
                        input #q.search-input type="search" name="q" value=(query)
                            placeholder=(c.t("sok.placeholder"))
                            autocomplete="off" enterkeyhint="search";
                    }
                    button.btn.btn-primary type="submit" { (c.t("sok.submit")) }
                    @if !query.is_empty() {
                        a.btn href="/sok" { (c.t("sok.clear")) }
                    }
                }
                p.field-hint.js-only { (c.t("sok.hint_a")) " " kbd { "/" } " " (c.t("sok.hint_b")) }
            }
            @if intro {
                div.card.glass.narrow.intro {
                    h2.card-title { (c.t("sok.intro.title")) }
                    p { (c.t("sok.intro.text")) }
                    p.muted.intro-try { (c.t("sok.try")) }
                    div.chips.chips-left { @for q in REAL_EXAMPLES { a.chip href=(sok_url(q, None)) { (q) } } }
                }
            } @else {
            p.result-count role="status" { (count_text) }
            div.table-wrap {
                table.row-link {
                    caption.sr-only { (c.t("sok.caption")) }
                    thead {
                        tr {
                            (sortable_th(c, c.t("sok.col.company"), "name", query, sort, dir, false, false))
                            th.hide-sm scope="col" { (c.t("sok.col.city")) }
                            (sortable_th(c, &c.tf("sok.col.revenue", &[unit]), "revenue", query, sort, dir, true, false))
                            (sortable_th(c, &c.tf("sok.col.result", &[unit]), "result", query, sort, dir, true, true))
                            th scope="col" { (c.t("sok.col.risk")) }
                        }
                    }
                    tbody {
                        @for co in results.iter() {
                            tr {
                                td.top {
                                    a.company-link href=(format!("/foretag/{}", co.org_number)) { (co.name) }
                                    div.org-sub { (co.org_number) span.city-sub { (co.city) } }
                                }
                                td.top.hide-sm { (co.city) }
                                td.top.right.num.mono { (c.int(co.financials.revenue[4])) }
                                td.top.right.num.mono.hide-sm.neg[co.financials.result[4] < 0] { (c.int(co.financials.result[4])) }
                                td.top { (risk_pill(c, co.risk_level)) }
                            }
                        }
                        @if let Some(o) = live {
                            tr {
                                td.top {
                                    a.company-link href=(format!("/foretag/{}", o.organisationsnummer)) { (o.namn) }
                                    div.org-sub { (o.formatted_number()) " · Bolagsverket" span.city-sub { (o.postort.clone().unwrap_or_default()) } }
                                }
                                td.top.hide-sm { (o.postort.clone().unwrap_or_default()) }
                                td.top.right.num.mono { "—" }
                                td.top.right.num.mono.hide-sm { "—" }
                                td.top { "—" }
                            }
                        }
                        @for h in reg_hits { (registry_row(c, h)) }
                        @if results.is_empty() && live.is_none() && reg_hits.is_empty() {
                            tr {
                                td colspan="5" {
                                    div.empty-state {
                                        strong { (c.t("sok.empty.title")) }
                                        p { (c.t("sok.empty.text")) }
                                        div.chips {
                                            @if c.demo {
                                                a.chip href=(sok_url("Göteborg", None)) { "Göteborg" }
                                                a.chip href=(sok_url("Uppsala", None)) { "Uppsala" }
                                                a.chip href=(sok_url("559108-7721", None)) { "559108-7721" }
                                                a.chip href="/sok" { (c.t("sok.empty.all")) }
                                            } @else {
                                                @for q in REAL_EXAMPLES { a.chip href=(sok_url(q, None)) { (q) } }
                                            }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            @if live.is_some() {
                p.note { (c.t("sok.note.live")) }
            } @else if reg_hits.is_empty() {
                @if c.demo { p.note { (example_badge(c)) " " (c.t("sok.note.example")) } }
            } @else {
                p.note { (c.t("sok.note.registry")) }
                @if registry.is_some_and(|r| r.more) { p.note { (c.tf("sok.registry_more", &[&reg_hits.len().to_string()])) } }
                @if c.demo && !results.is_empty() { p.note { (example_badge(c)) " " (c.t("sok.note.example")) } }
            }
            }
            // Filtrado "en vivo": reenvía el formulario al escribir (con retardo).
            script { (PreEscaped(SEARCH_SCRIPT)) }
        },
    )
}

// ───────────────────────── Ficha de empresa (ejemplo) ─────────────────────────

/// Estado de la tarjeta de comparación con el sector al renderizar la ficha.
pub enum Bench {
    /// SCB desactivado: medianas de EJEMPLO.
    Example,
    /// Resultado de SCB ya en caché (o "sin datos").
    Ready(Option<SectorMedians>),
    /// Hay que preguntar a SCB: esqueleto de carga y fragmento diferido.
    Pending,
}

const BENCH_SCRIPT: &str = r#"(function(){var c=document.querySelector('[data-fragment]');if(!c)return;var u=c.getAttribute('data-fragment');
function fail(){c.removeAttribute('aria-busy');var s=c.querySelectorAll('.skeleton-row');for(var i=0;i<s.length;i++)s[i].hidden=true;var e=c.querySelector('.bench-error');if(e)e.hidden=false;}
fetch(u,{headers:{'Accept':'text/html'}}).then(function(r){if(!r.ok)throw 0;return r.text();}).then(function(h){c.innerHTML=h;c.removeAttribute('aria-busy');}).catch(fail);})();"#;

pub fn company_page(c: &Ctx, company: &Company, tab: &str, bench: &Bench) -> Markup {
    let f = &company.financials;
    let growth = (f.revenue[4] as f64 / f.revenue[3] as f64 - 1.0) * 100.0;
    let margin = f.result[4] as f64 / f.revenue[4] as f64 * 100.0;
    let solidity = f.equity[4] as f64 / f.total_assets[4] as f64 * 100.0;
    let status = c.t(company.status);

    layout(
        c,
        company.name,
        html! {
            a.back href="/sok" { (icon_sized("arrow-left", "icon-sm")) (c.t("co.back")) }

            div.company-head {
                div {
                    h1.company-name { (company.name) }
                    div.company-meta {
                        (term(c.t("co.org_nr"), c.t("tip.org_nr"))) " " (company.org_number)
                        button.copy-btn type="button" data-copy=(company.org_number) data-tip=(c.t("co.copy_org"))
                            aria-label=(c.tf("co.copy_org_aria", &[company.org_number])) {
                            span.icon-copy { (icon_sized("copy", "icon-sm")) }
                            span.icon-check { (icon_sized("check", "icon-sm")) }
                        }
                        " · " (c.t(company.legal_form)) " · " (company.city)
                    }
                }
                div.pills {
                    (status_pill(status))
                    (risk_pill(c, company.risk_level))
                }
            }

            dl.kpis {
                (kpi(c.t("kpi.revenue"), c.t("tip.revenue"), &c.money(f.revenue[4]), false,
                    html! {
                        (icon_sized(if growth >= 0.0 { "arrow-up-right" } else { "arrow-down-right" }, "icon-sm"))
                        (sr(c.t(if growth >= 0.0 { "kpi.increase" } else { "kpi.decrease" })))
                        (c.tf("kpi.vs_year", &[&c.dec(growth.abs(), 1), "2023"]))
                    },
                    if growth >= 0.0 { "up" } else { "down" }))
                (kpi(c.t("kpi.result"), c.t("tip.result"), &c.money(f.result[4]), f.result[4] < 0,
                    html! { (c.t("term.margin")) " " (c.pct1(margin)) }, ""))
                (kpi(c.t("term.solidity"), c.t("tip.solidity"), &c.pct1(solidity), false,
                    html! { (c.tf("kpi.equity_is", &[&c.money(f.equity[4])])) }, ""))
                (kpi(c.t("kpi.employees"), c.t("tip.employees"), company.employee_range, false, html! { (c.t("kpi.scb_interval")) }, ""))
            }

            (company_tabs(c, company, tab, bench))
        },
    )
}

fn kpi(label: &str, label_tip: &str, value: &str, negative: bool, detail: Markup, detail_class: &str) -> Markup {
    html! {
        div.kpi {
            dt.kpi-label { (term(label, label_tip)) }
            dd.kpi-value.neg[negative] { (value) }
            dd class={"kpi-detail " (detail_class)} { (detail) }
        }
    }
}

/// Pestañas de la ficha de una empresa real.
pub const LIVE_TABS: [(&str, &str); 4] = [("ov", "tab.overview"), ("fin", "tab.financials"), ("ppl", "tab.people"), ("dat", "tab.data")];

const SUB_TABS: [(&str, &str); 4] = [("ov", "tab.overview"), ("fin", "tab.financials"), ("ppl", "tab.people"), ("ai", "tab.summary")];

fn company_tabs(c: &Ctx, company: &Company, tab: &str, bench: &Bench) -> Markup {
    // Pestaña desconocida → resumen general (la inicial del original).
    let active = SUB_TABS.iter().find(|(id, _)| *id == tab).map(|(id, _)| *id).unwrap_or("ov");
    html! {
        nav.tabs #vyer aria-label=(c.t("co.views")) {
            @for (id, label) in SUB_TABS.iter() {
                a.tab aria-current=[(*id == active).then_some("page")]
                    href=(format!("/foretag/{}?tab={}#vyer", company.org_number, id)) { (c.t(label)) }
            }
        }
        @match active {
            "fin" => { (financials(c, company)) }
            "ppl" => { (people(c, company)) }
            "ai" => { (summary(c, company)) }
            _ => { (overview(c, company, bench)) }
        }
    }
}

fn overview(c: &Ctx, company: &Company, bench: &Bench) -> Markup {
    let revenue = &company.financials.revenue;
    let unit = c.t("unit.tkr");
    html! {
        div.overview-grid {
            div.card {
                h2.card-title {
                    (c.tf("ov.revenue_title", &[unit]))
                    (info_btn(c.t("ov.about_chart"), c.t("ov.chart_tip")))
                }
                (revenue_chart(c, revenue))
                div.legend aria-hidden="true" {
                    span.legend-item { span.swatch.past {} (c.t("ov.legend.past")) }
                    span.legend-item { span.swatch.fill {} (c.t("ov.legend.latest")) }
                    span.legend-item { (c.tf("ov.legend.values_in", &[c.t("unit.mkr")])) }
                }
                details.chart-data {
                    summary { (c.t("chart.show_table")) }
                    div.table-wrap {
                        table {
                            caption.sr-only { (c.tf("ov.revenue_caption", &[unit])) }
                            thead { tr { th scope="col" { (c.t("ov.col.year")) } th.right scope="col" { (unit) } } }
                            tbody {
                                @for (y, v) in FINANCIAL_YEARS.iter().zip(revenue.iter()) {
                                    tr { th scope="row" { (y) } td.right.num.mono { (c.int(*v)) } }
                                }
                            }
                        }
                    }
                }
            }
            div.card {
                h2.card-title {
                    (c.t("ov.bench_title"))
                    (info_btn(c.t("ov.about_bench"), c.t("ov.bench_tip")))
                }
                @match bench {
                    Bench::Pending => {
                        div #bench-body aria-busy="true" data-fragment=(format!("/foretag/{}/benchmarks", company.org_number)) {
                            (sr(c.t("ov.loading_bench")))
                            @for _ in 0..3 {
                                div.skeleton-row aria-hidden="true" { div.skeleton-line {} div.skeleton-bar {} }
                            }
                            p.bench-error hidden { (c.t("ov.bench_error")) }
                            noscript { (benchmark_fragment(c, company, None, Some(c.t("ov.bench_nojs")))) }
                        }
                        script { (PreEscaped(BENCH_SCRIPT)) }
                    }
                    Bench::Ready(m) => { div #bench-body { (benchmark_fragment(c, company, m.as_ref(), None)) } }
                    Bench::Example => { div #bench-body { (benchmark_fragment(c, company, None, None)) } }
                }
            }
        }

        div.card.mt-4 {
            h2.card-title { (c.t("ov.signals")) }
            ul.alert-list {
                @for a in company.alerts.iter() {
                    (alert_row(c, a.severity, html! { (c.t(a.text)) }))
                }
            }
        }
        p.note { (c.t("ov.activity")) ": " (company.sni_code) " " (c.t(company.sni_text)) }
    }
}

fn financials(c: &Ctx, company: &Company) -> Markup {
    let f = &company.financials;
    let rows: [(&str, &[i64; 5]); 4] = [
        ("fin.revenue", &f.revenue),
        ("fin.result", &f.result),
        ("fin.equity", &f.equity),
        ("fin.assets", &f.total_assets),
    ];
    let unit = c.t("unit.tkr");
    html! {
        div.table-wrap {
            table.fin-table {
                caption.sr-only { (c.tf("fin.caption", &[unit])) }
                thead {
                    tr {
                        th scope="col" { (unit) }
                        @for y in FINANCIAL_YEARS.iter() { th.right scope="col" { (y) } }
                    }
                }
                tbody {
                    @for (label, values) in rows.iter() {
                        tr {
                            th scope="row" { (c.t(label)) }
                            @for v in values.iter() {
                                td.right.num.mono.neg[*v < 0] { (c.int(*v)) }
                            }
                        }
                    }
                }
            }
        }
        p.note { (c.t("fin.source")) " " (example_badge(c)) }
        div.mt-3 {
            button.btn type="button" disabled title=(c.t("fin.download_tip")) { (c.t("fin.download")) }
        }
    }
}

fn people(c: &Ctx, company: &Company) -> Markup {
    html! {
        div.table-wrap {
            table {
                caption.sr-only { (c.t("ppl.caption")) }
                thead { tr { th scope="col" { (c.t("ppl.role")) } th scope="col" { (c.t("ppl.name")) } } }
                tbody {
                    @for p in company.people.iter() {
                        tr { td { (c.t(p.role)) } td { (p.name) } }
                    }
                }
            }
        }
        p.mt-3 { (c.t("ppl.signing")) ": " strong { (c.t(company.signing)) } }
        p.note { (c.t("ppl.note")) " " (example_badge(c)) }
    }
}

fn summary(c: &Ctx, company: &Company) -> Markup {
    html! {
        div.card {
            h2.card-title { (c.t("sum.title")) }
            p.summary-text { (generate_example_summary(c.lang, company)) }
            p.note { (c.t("sum.note")) " " (example_badge(c)) }
        }
    }
}

pub fn company_not_found_page(c: &Ctx) -> Markup {
    layout(
        c,
        c.t("nf.company.title"),
        html! {
            div.page-head {
                h1.page-title { (c.t("nf.company.title")) }
                p.lead { (c.t("nf.company.text")) }
            }
            a.btn.btn-primary href="/sok" { (icon_sized("search", "icon-sm")) (c.t("nf.to_search")) }
        },
    )
}

pub fn not_found_page(c: &Ctx) -> Markup {
    layout(
        c,
        c.t("nf.page.title"),
        html! {
            div.page-head {
                h1.page-title { (c.t("nf.page.title")) }
                p.lead { (c.t("nf.page.text")) }
            }
            a.btn.btn-primary href="/sok" { (icon_sized("search", "icon-sm")) (c.t("nf.to_search")) }
        },
    )
}

/// Página de error genérica (403, etc.) con el mismo diseño.
pub fn message_page(c: &Ctx, title: &str, text: &str) -> Markup {
    layout(
        c,
        title,
        html! {
            div.page-head {
                h1.page-title { (title) }
                p.lead { (text) }
            }
            a.btn.btn-primary href="/sok" { (icon_sized("search", "icon-sm")) (c.t("nf.to_search")) }
        },
    )
}

// ───────────────────────── Ficha real (Bolagsverket) ─────────────────────────

/// Estado de la sección de cuentas de una ficha real al renderizar: ya en caché, o pendiente de descargar.
pub enum FinState {
    /// Cuentas ya en caché (o "sin cuentas") y medianas del sector (o "sin datos").
    Ready(Option<Financials>, Option<SectorMedians>),
    Pending,
}

/// Ficha con datos REALES de Bolagsverket (API gratuito). Los textos de la fuente (actividad, forma jurídica)
/// están en sueco y se muestran tal cual.
pub fn live_profile_page(c: &Ctx, o: &Organisation, fin: &FinState, following: bool, tab: &str) -> Markup {
    let tab = LIVE_TABS.iter().find(|(id, _)| *id == tab).map(|(id, _)| *id).unwrap_or("ov");
    let address = [o.gatuadress.clone(), Some([o.postnummer.clone(), o.postort.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "))]
        .into_iter()
        .flatten()
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    layout(
        c,
        &o.namn,
        html! {
            a.back href="/sok" { (icon_sized("arrow-left", "icon-sm")) (c.t("co.back")) }

            div.company-head {
                div {
                    h1.company-name { (o.namn) }
                    div.company-meta {
                        (term(c.t("co.org_nr"), c.t("tip.org_nr"))) " " (o.formatted_number())
                        button.copy-btn type="button" data-copy=(o.formatted_number()) data-tip=(c.t("co.copy_org"))
                            aria-label=(c.tf("co.copy_org_aria", &[&o.formatted_number()])) {
                            span.icon-copy { (icon_sized("copy", "icon-sm")) }
                            span.icon-check { (icon_sized("check", "icon-sm")) }
                        }
                        " · " (o.organisationsform)
                        @if let Some(ort) = &o.postort { " · " (ort) }
                    }
                }
                div.pills {
                    @if o.aktiv {
                        (status_pill(c.t("status.active")))
                    } @else {
                        span.pill.pill-bad {
                            span.pill-dot aria-hidden="true" {}
                            @if let Some(d) = &o.avregistreringsdatum { (c.tf("live.deregistered", &[d])) } @else { (c.t("live.inactive")) }
                        }
                    }
                    @for f in o.forfaranden.iter() {
                        span.pill.pill-bad { span.pill-dot aria-hidden="true" {} (f) }
                    }
                }
                (crate::views_tools::follow_button(c, &o.organisationsnummer, following))
            }

            nav.tabs #vyer aria-label=(c.t("co.views")) {
                @for (id, label) in LIVE_TABS.iter() {
                    a.tab aria-current=[(*id == tab).then_some("page")] href=(format!("/foretag/{}?tab={}#vyer", o.organisationsnummer, id)) { (c.t(label)) }
                }
            }

            @if tab == "ov" {
            div.card {
                h2.card-title { (c.t("live.facts")) }
                dl.facts {
                    dt { (c.t("live.address")) } dd { @if address.is_empty() { "—" } @else { (address) } }
                    dt { (c.t("live.registered")) } dd { @if o.registreringsdatum.is_empty() { "—" } @else { (o.registreringsdatum) } }
                    dt { (term(c.t("live.sni"), c.t("tip.sni"))) }
                    dd {
                        @if o.sni.is_empty() { "—" } @else {
                            @for (i, (kod, text)) in o.sni.iter().enumerate() {
                                @if i > 0 { " · " }
                                span.mono { (kod) } @if !text.is_empty() { " " (text) }
                            }
                        }
                    }
                    dt { (c.t("live.activity")) } dd { (o.verksamhetsbeskrivning.clone().unwrap_or_else(|| "—".to_string())) }
                }
                @if c.lang != Lang::Sv {
                    p.note { (c.t("live.swedish_note")) }
                }
            }
            }
            // Resumen: lo que ya esté en caché sale al instante. El resto de pestañas piden su contenido en segundo plano
            // (con las cuentas ya guardadas es inmediato).
            @match (tab, fin) {
                (t, FinState::Ready(f, m)) if t == "ov" => { div #fin-body { (financials_fragment(c, o, f.as_ref(), m.as_ref())) } }
                _ => {
                    div #fin-body aria-busy="true" data-fragment=(if tab == "ov" { format!("/foretag/{}/bokslut", o.organisationsnummer) } else { format!("/foretag/{}/bokslut?tab={}", o.organisationsnummer, tab) }) {
                        div.card.mt-4 {
                            h2.card-title { (c.t(if tab == "ov" { "as.title" } else { "live.financials" })) }
                            (sr(c.t("live.loading")))
                            @for _ in 0..4 {
                                div.skeleton-row aria-hidden="true" { div.skeleton-line {} div.skeleton-bar {} }
                            }
                            p.bench-error hidden { (c.t("live.error")) }
                        }
                    }
                    script { (PreEscaped(BENCH_SCRIPT)) }
                }
            }
            p.note { (c.t("live.source")) }
        },
    )
}

pub fn risk_pill_opt(c: &Ctx, level: Option<Severity>) -> Markup {
    match level {
        Some(l) => risk_pill(c, l),
        None => html! { span.pill.pill-user { span.pill-dot aria-hidden="true" {} (c.t("risk.unknown")) } },
    }
}

/// Texto de una señal en el idioma de la persona, con las cifras reales de la empresa.
fn signal_text(c: &Ctx, s: &Signal) -> String {
    let money = |v: i64| c.money(v);
    match s {
        Signal::Insolvency(t) => c.tf("sig.insolvency", &[t]),
        Signal::Deregistered(d) => c.tf("sig.deregistered", &[d]),
        Signal::NoFilings => c.t("sig.no_filings").to_string(),
        Signal::StaleReport(d) => c.tf("sig.stale", &[d]),
        Signal::NegativeEquity(e) => c.tf("sig.negative_equity", &[&money(*e)]),
        Signal::LossStreak(a, b) => c.tf("sig.loss_streak", &[&money(*a), &money(*b)]),
        Signal::Loss(r) => c.tf("sig.loss", &[&money(*r)]),
        Signal::LowSolidity(p) => c.tf("sig.low_solidity", &[&c.pct1(*p)]),
        Signal::LowLiquidity(p) => c.tf("sig.low_liquidity", &[&c.pct1(*p)]),
        Signal::CriticalLiquidity(p) => c.tf("sig.critical_liquidity", &[&c.pct1(*p)]),
        Signal::CapitalLost(equity, capital) => c.tf("sig.capital_lost", &[&c.money(*equity), &c.money(*capital)]),
        Signal::WeakInterestCover(x) => c.tf("sig.weak_interest", &[&c.dec(*x, 1)]),
        Signal::SolidityBelowMedian(v, m) => c.tf("sig.solidity_below", &[&c.pct1(*v), &c.pct1(*m)]),
        Signal::SolidityAboveMedian(v, m) => c.tf("sig.solidity_above", &[&c.pct1(*v), &c.pct1(*m)]),
        Signal::MarginBelowMedian(v, m) => c.tf("sig.margin_below", &[&c.pct1(*v), &c.pct1(*m)]),
        Signal::MarginAboveMedian(v, m) => c.tf("sig.margin_above", &[&c.pct1(*v), &c.pct1(*m)]),
        Signal::RevenueDrop(p, from, to) => c.tf("sig.revenue_drop", &[&c.pct1(*p), from, to]),
        Signal::RevenueGrowth(p, from, to) => c.tf("sig.revenue_growth", &[&c.pct1(*p), from, to]),
        Signal::Profitable(n) => c.tf("sig.profitable", &[&n.to_string()]),
    }
}

/// Resumen en lenguaje natural construido con reglas sobre las cifras reales (no usa ningún modelo de IA).
fn live_summary(c: &Ctx, fin: Option<&Financials>, medians: Option<&SectorMedians>, level: Option<Severity>) -> String {
    let Some(latest) = fin.and_then(|f| f.latest()) else { return c.t("sum.live.no_data").to_string() };
    let previous = fin.and_then(|f| f.previous());
    let mut parts: Vec<String> = Vec::new();

    if let Some(rev) = latest.revenue {
        let growth = previous.and_then(|p| p.revenue).filter(|b| *b > 0).map(|b| (rev as f64 / b as f64 - 1.0) * 100.0);
        let prev_label = previous.map(|p| p.label.as_str()).unwrap_or("");
        parts.push(match growth {
            Some(g) if g >= 1.0 => c.tf("sum.live.rev_up", &[&latest.label, &c.money(rev), &c.pct1(g), prev_label]),
            Some(g) if g <= -1.0 => c.tf("sum.live.rev_down", &[&latest.label, &c.money(rev), &c.pct1(-g), prev_label]),
            _ => c.tf("sum.live.rev", &[&latest.label, &c.money(rev)]),
        });
    }
    if let Some(res) = latest.result {
        parts.push(match latest.margin() {
            Some(m) if res >= 0 => c.tf("sum.live.profit", &[&c.money(res), &c.pct1(m)]),
            Some(m) => c.tf("sum.live.loss", &[&c.money(-res), &c.pct1(m)]),
            None => c.tf("sum.live.result_only", &[&c.money(res)]),
        });
    }
    if let Some(s) = latest.solidity() {
        parts.push(match medians {
            Some(m) => c.tf("sum.live.solidity_vs", &[&c.pct1(s), c.t(if s >= m.solidity { "bench.above" } else { "bench.below" }), &c.pct1(m.solidity)]),
            None => c.tf("sum.live.solidity", &[&c.pct1(s)]),
        });
    }
    if let Some(liq) = latest.liquidity() {
        parts.push(match medians {
            Some(m) => c.tf("sum.live.liquidity_vs", &[&c.pct1(liq), c.t(if liq >= m.liquidity { "bench.above" } else { "bench.below" }), &c.pct1(m.liquidity)]),
            None => c.tf("sum.live.liquidity", &[&c.pct1(liq)]),
        });
    }
    if let Some(l) = level {
        parts.push(
            c.t(match l {
                Severity::Good => "sum.live.risk_good",
                Severity::Warn => "sum.live.risk_warn",
                Severity::Bad => "sum.live.risk_bad",
            })
            .to_string(),
        );
    }
    parts.join(" ")
}

/// Un decimal: las barras de comparación muestran el número tal cual, y las cifras reales traen muchos decimales.
fn round1(v: f64) -> f64 {
    (v * 10.0).round() / 10.0
}

/// Tarjeta de valoración: nivel de riesgo, resumen, señales con sus cifras y comparación con el sector.
fn assessment_card(c: &Ctx, org: &Organisation, fin: Option<&Financials>, medians: Option<&SectorMedians>) -> Markup {
    let today = crate::util::now_iso();
    let a = analysis::assess(org, fin, medians, &today[..10]);
    let latest = fin.and_then(|f| f.latest());
    let summary = live_summary(c, fin, medians, a.level);
    let bars: Vec<(&str, &str, BenchmarkPair, f64)> = match (latest, medians) {
        (Some(y), Some(m)) => [
            y.margin().map(|v| ("term.margin", "tip.margin", BenchmarkPair { value: round1(v), median: round1(m.margin) }, 30.0)),
            y.solidity().map(|v| ("term.solidity", "tip.solidity.bench", BenchmarkPair { value: round1(v), median: round1(m.solidity) }, 80.0)),
            y.liquidity().map(|v| ("term.liquidity", "tip.liquidity", BenchmarkPair { value: round1(v), median: round1(m.liquidity) }, 400.0)),
        ]
        .into_iter()
        .flatten()
        .collect(),
        _ => vec![],
    };
    html! {
        div.card.mt-4 {
            h2.card-title { (c.t("as.title")) (info_btn(c.t("as.about"), c.t("as.tip"))) }
            div.assess-head {
                (risk_pill_opt(c, a.level))
                @if let Some(y) = latest { span.muted { (c.tf("as.based_on", &[y.label.as_str()])) } }
            }
            p.summary-text { (summary) }
            ul.alert-list.mt-3 {
                @for s in &a.signals { (alert_row(c, s.severity(), html! { (signal_text(c, s)) })) }
                @if a.signals.is_empty() { li { span { (c.t("as.no_signals")) } } }
            }
            @if !bars.is_empty() {
                h3.subtitle { (c.t("ov.bench_title")) }
                @for (label, tip, pair, scale) in &bars { (benchmark_row(c, label, tip, *pair, *scale)) }
                (bench_legend(c))
                @if let Some(m) = medians {
                    @let label = if m.sni_label.is_empty() { String::new() } else { format!(" ({})", m.sni_label) };
                    @let size = if m.size_class == "TOT" { c.t("bench.all_sizes").to_string() } else { c.tf("bench.size", &[&m.size_class.replace("001", "0")]) };
                    p.note { (c.tf("bench.note.real", &[m.sni_code.as_str(), &label, &size, m.year.as_str()])) }
                }
            } @else if latest.is_some() {
                p.note { (c.t("bench.none")) }
            }
            p.note { (c.t("sum.live.note")) }
        }
    }
}

/// Valoración y cifras reales de las cuentas anuales: tarjeta de riesgo, resumen del último año, gráfico y tabla.
/// `fin = None` = la empresa no ha presentado cuentas anuales digitales.
pub fn financials_fragment(c: &Ctx, org: &Organisation, fin: Option<&Financials>, medians: Option<&SectorMedians>) -> Markup {
    let usable = fin.filter(|f| f.latest().is_some());
    html! {
        (assessment_card(c, org, usable, medians))
        div.card.mt-4 {
            h2.card-title {
                (c.t("live.financials"))
                (info_btn(c.t("live.about_financials"), c.t("live.financials_tip")))
            }
            @if let Some(fin) = usable {
                @if fin.consolidated { p.note { (c.t("fin.esef_note")) } }
                (financials_body(c, fin))
            } @else {
                p.muted { (c.t("bok.none")) }
                p.note { (c.t("bok.none_note")) }
                p.note { (c.t("bok.none_why")) }
                h3.card-subtitle { (c.t("bok.none_examples")) }
                div.chips.chips-left {
                    @for n in crate::views_fin::EXAMPLES_WITH_ACCOUNTS {
                        @if n != org.organisationsnummer { a.chip href=(format!("/foretag/{n}")) { (format_orgnr(n)) } }
                    }
                }
            }
        }
    }
}

/// Cifras del último año, gráfico de facturación y tabla de cinco ejercicios.
fn financials_body(c: &Ctx, fin: &Financials) -> Markup {
    let latest = fin.latest().expect("la llamada comprueba que hay un ejercicio");
    let unit = c.t("unit.tkr");
    let money = |v: Option<i64>| v.map(|n| c.money(n)).unwrap_or_else(|| "—".to_string());
    let chart: Vec<(&str, i64)> = fin.years.iter().filter_map(|y| y.revenue.map(|r| (y.label.as_str(), r))).collect();
    let has_revenue = chart.iter().any(|(_, v)| *v > 0);
    let (chart_labels, chart_values): (Vec<&str>, Vec<i64>) = chart.into_iter().unzip();
    let rows: [(&str, Vec<Option<i64>>); 4] = [
        ("fin.revenue", fin.years.iter().map(|y| y.revenue).collect()),
        ("fin.result", fin.years.iter().map(|y| y.result).collect()),
        ("fin.equity", fin.years.iter().map(|y| y.equity).collect()),
        ("fin.assets", fin.years.iter().map(|y| y.assets).collect()),
    ];
    html! {
        dl.stats {
            div.stat {
                dt { (c.tf("bok.revenue_year", &[latest.label.as_str()])) }
                dd {
                    (money(latest.revenue))
                    @if let (Some(now), Some(before)) = (latest.revenue, fin.previous().and_then(|p| p.revenue)) {
                        @if before > 0 {
                            @let change = (now as f64 / before as f64 - 1.0) * 100.0;
                            span.sub.up[change >= 0.0].down[change < 0.0] {
                                (icon_sized(if change >= 0.0 { "arrow-up-right" } else { "arrow-down-right" }, "icon-sm"))
                                (sr(c.t(if change >= 0.0 { "kpi.increase" } else { "kpi.decrease" })))
                                (c.tf("kpi.vs_year", &[&c.dec(change.abs(), 1), fin.previous().map(|p| p.label.as_str()).unwrap_or("")]))
                            }
                        }
                    }
                }
            }
            div.stat {
                dt { (c.t("kpi.result")) }
                dd.neg[latest.result.is_some_and(|r| r < 0)] {
                    (money(latest.result))
                    @if let Some(m) = latest.margin() { span.sub { (term(c.t("term.margin"), c.t("tip.margin"))) " " (c.pct1(m)) } }
                }
            }
            div.stat { dt { (c.t("fin.equity")) } dd.neg[latest.equity.is_some_and(|e| e < 0)] { (money(latest.equity)) } }
            div.stat {
                dt { (term(c.t("term.solidity"), c.t("tip.solidity"))) }
                dd { (latest.solidity().map(|s| c.pct1(s)).unwrap_or_else(|| "—".to_string())) }
            }
        }
        @if has_revenue {
            (bar_chart(c, &chart_labels, &chart_values))
        } @else {
            p.muted { (c.t("bok.no_revenue")) }
        }
        div.table-wrap.mt-3 {
            table.fin-table {
                caption.sr-only { (c.tf("fin.caption", &[unit])) }
                thead {
                    tr {
                        th scope="col" { (unit) }
                        @for y in fin.years.iter() { th.right scope="col" { (y.label) } }
                    }
                }
                tbody {
                    @for (label, values) in rows.iter() {
                        tr {
                            th scope="row" { (c.t(label)) }
                            @for v in values.iter() {
                                @match v {
                                    Some(n) => { td.right.num.mono.neg[*n < 0] { (c.int(*n)) } }
                                    None => { td.right.num.mono.muted { "—" } }
                                }
                            }
                        }
                    }
                }
            }
        }
        p.note { (c.tf(if fin.consolidated { "bok.source_esef" } else { "bok.source" }, &[latest.period_end.as_str()])) }
    }
}

pub fn live_error_page(c: &Ctx) -> Markup {
    layout(
        c,
        c.t("live.err.title"),
        html! {
            div.page-head {
                h1.page-title { (c.t("live.err.title")) }
                p.lead { (c.t("live.err.text")) }
            }
            a.btn.btn-primary href="/sok" { (icon_sized("search", "icon-sm")) (c.t("nf.to_search")) }
        },
    )
}

// ───────────────────────── Otras pantallas ─────────────────────────

pub fn likviditet_page(c: &Ctx) -> Markup {
    let unit = c.t("unit.tkr");
    layout(
        c,
        c.t("nav.liquidity"),
        html! {
            div.page-head { h1.page-title { (c.t("liq.title")) } }
            div.card.narrow {
                h2.card-title {
                    (c.tf("liq.card_title", &[unit]))
                    (info_btn(c.t("ov.about_chart"), c.t("liq.chart_tip")))
                }
                (cash_flow_chart(c, &EXAMPLE_CASH_WEEKS, EXAMPLE_CASH_START_BALANCE))
            }
            div.card.narrow.mt-3 {
                ul.alert-list { (alert_row(c, Severity::Warn, html! { (c.t("liq.alert")) })) }
            }
            p.note { (example_badge(c)) " " (c.t("liq.note")) }
        },
    )
}

pub fn sie_page(c: &Ctx) -> Markup {
    layout(
        c,
        c.t("nav.sie"),
        html! {
            div.page-head { h1.page-title { (c.t("sie.title")) } }
            div.dropzone aria-disabled="true" {
                (icon("upload"))
                strong { (c.t("sie.drop")) }
                p { (c.t("sie.formats")) }
                button.btn type="button" disabled { (c.t("sie.choose")) }
                p.fine { (c.t("sie.inactive")) }
            }
            div.card.narrow.mt-3 {
                h2.card-title { (c.t("sie.preview")) }
                div.scroll-x {
                    table {
                        caption.sr-only { (c.t("sie.caption")) }
                        thead {
                            tr {
                                th scope="col" { (c.t("sie.col.account")) }
                                th scope="col" { (c.t("sie.col.name")) }
                                th.right scope="col" { (c.t("sie.col.balance")) }
                            }
                        }
                        tbody {
                            @for row in EXAMPLE_SIE_PREVIEW.iter() {
                                tr {
                                    td.mono { (row.account) }
                                    td { (c.t(row.name)) }
                                    td.right.num.mono.neg[row.balance_sek < 0] { (c.int(row.balance_sek)) }
                                }
                            }
                        }
                    }
                }
            }
            p.note { (example_badge(c)) " " (c.t("sie.note")) }
        },
    )
}

pub fn fakturor_page(c: &Ctx) -> Markup {
    let sum = |key: &str| -> i64 { EXAMPLE_INVOICES.iter().filter(|i| i.status == key).map(|i| i.amount_sek).sum() };
    let sek = c.t("unit.sek");
    layout(
        c,
        c.t("nav.invoices"),
        html! {
            div.page-head { h1.page-title { (c.t("inv.title")) } }
            dl.stats {
                div.stat { dt { (c.t("inv.overdue")) } dd.neg { (c.int(sum("inv.overdue"))) " " (sek) } }
                div.stat { dt { (c.t("inv.unpaid")) } dd { (c.int(sum("inv.unpaid"))) " " (sek) } }
                div.stat { dt { (c.t("inv.paid")) } dd { (c.int(sum("inv.paid"))) " " (sek) } }
            }
            div.table-wrap {
                table {
                    caption.sr-only { (c.t("inv.title")) }
                    thead {
                        tr {
                            th scope="col" { (c.t("inv.col.no")) }
                            th scope="col" { (c.t("inv.col.customer")) }
                            th.right scope="col" { (c.tf("inv.col.amount", &[sek])) }
                            th scope="col" { (c.t("inv.col.due")) }
                            th scope="col" { (c.t("inv.col.status")) }
                        }
                    }
                    tbody {
                        @for inv in EXAMPLE_INVOICES.iter() {
                            tr {
                                td.mono { (inv.number) }
                                td { (inv.customer) }
                                td.right.num.mono { (c.int(inv.amount_sek)) }
                                td.num { (inv.due_date) }
                                td { span class={"pill pill-" (sev_class(inv.severity))} { span.pill-dot aria-hidden="true" {} (c.t(inv.status)) } }
                            }
                        }
                    }
                }
            }
            p.note { (example_badge(c)) " " (c.t("inv.note")) }
        },
    )
}
