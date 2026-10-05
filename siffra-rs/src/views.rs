//! Layout, componentes y páginas. Equivale a `src/components/*` y `src/app/**/page.tsx`.

use maud::{html, Markup, PreEscaped, DOCTYPE};

use crate::bolagsverket::Organisation;
use crate::format::{format_int, format_tkr, js_round};
use crate::model::*;
use crate::scb::SectorMedians;
use crate::summary::generate_example_summary;

// ───────────────────────── Iconos (trazo único: 1.75, redondeado) ─────────────────────────

fn icon(name: &str) -> Markup {
    icon_sized(name, "icon")
}

fn icon_sized(name: &str, class: &str) -> Markup {
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
        _ => "",
    };
    html! {
        svg class=(class) viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.75"
            stroke-linecap="round" stroke-linejoin="round" aria-hidden="true" focusable="false" {
            (PreEscaped(paths))
        }
    }
}

fn sr(text: &str) -> Markup {
    html! { span.sr-only { (text) } }
}

/// Término con explicación: botón con subrayado punteado; el tooltip sale con hover, foco o toque.
fn term(label: &str, tip: &str) -> Markup {
    html! { button.term type="button" data-tip=(tip) { (label) } }
}

/// Botón "i" junto a un título, con zona táctil ampliada.
fn info_btn(label: &str, tip: &str) -> Markup {
    html! { button.info type="button" data-tip=(tip) aria-label=(label) { (icon_sized("info", "icon-sm")) } }
}

const TIP_SOLIDITET: &str = "Eget kapital i procent av summa tillgångar: hur stor del av bolaget som är egenfinansierad.";
const TIP_SOLIDITET_BENCH: &str = "Eget kapital i procent av summa tillgångar: hur stor del av bolaget som är egenfinansierad. SCB:s median räknar med justerat eget kapital.";
const TIP_MARGIN: &str = "Resultat efter finansiella poster i procent av omsättningen. SCB:s motsvarande mått heter nettomarginal.";
const TIP_LIQUIDITY: &str = "Omsättningstillgångar exklusive lager, i procent av kortfristiga skulder: förmågan att betala kortsiktiga skulder.";

// ───────────────────────── Layout ─────────────────────────

struct NavItem {
    href: &'static str,
    label: &'static str,
    short: &'static str,
    icon: &'static str,
    badge: Option<&'static str>,
}

struct NavGroup {
    heading: &'static str,
    items: &'static [NavItem],
}

const NAV: [NavGroup; 2] = [
    NavGroup {
        heading: "Företag",
        items: &[
            NavItem { href: "/sok", label: "Sök företag", short: "Sök", icon: "search", badge: None },
            NavItem { href: "/bevakning", label: "Bevakning", short: "Bevakning", icon: "bell", badge: Some("3") },
        ],
    },
    NavGroup {
        heading: "Min verksamhet",
        items: &[
            NavItem { href: "/likviditet", label: "Likviditetsprognos", short: "Likviditet", icon: "trend", badge: None },
            NavItem { href: "/sie", label: "Importera SIE", short: "SIE", icon: "upload", badge: None },
            NavItem { href: "/fakturor", label: "Fakturor", short: "Fakturor", icon: "receipt", badge: None },
        ],
    },
];

const DEFAULT_TITLE: &str = "Siffra — MVP";
const DESCRIPTION: &str = "Siffra (nombre de trabajo): inteligencia financiera de empresas para el mercado sueco. Proyecto en construcción — ver README para el estado real.";
const FONTS_URL: &str = "https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:wght@500;700&family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans:wght@400;500;600&display=swap";
const FAVICON: &str = "data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' viewBox='0 0 32 32'%3E%3Crect width='32' height='32' rx='7' fill='%230b6e75'/%3E%3Cpath d='M21 11.5c-1-1.6-2.8-2.5-5-2.5-2.9 0-4.8 1.5-4.8 3.6 0 5 10 2.7 10 7.6 0 2.3-2.2 3.8-5.2 3.8-2.4 0-4.4-1-5.4-2.7' fill='none' stroke='white' stroke-width='2.6' stroke-linecap='round'/%3E%3C/svg%3E";

/// Se ejecuta antes de pintar: aplica el tema guardado (sin parpadeo) y marca que hay JavaScript.
const HEAD_SCRIPT: &str = r#"(function(){var r=document.documentElement;r.classList.add('js');try{var t=localStorage.getItem('theme');if(t==='light'||t==='dark')r.dataset.theme=t;}catch(e){}})();"#;

const THEME_SCRIPT: &str = r#"(function(){var b=document.getElementById('theme-toggle');if(!b)return;var r=document.documentElement;
function cur(){return r.dataset.theme||(matchMedia('(prefers-color-scheme: dark)').matches?'dark':'light');}
function sync(){b.setAttribute('aria-pressed',cur()==='dark'?'true':'false');}
sync();
b.addEventListener('click',function(){var n=cur()==='dark'?'light':'dark';r.dataset.theme=n;try{localStorage.setItem('theme',n);}catch(e){}sync();});
matchMedia('(prefers-color-scheme: dark)').addEventListener('change',sync);})();"#;

/// Tooltips (`[data-tip]`), copiar al portapapeles (`[data-copy]`) y atajo "/" para ir al buscador.
/// Un único tooltip para toda la página: se muestra con hover (si hay ratón), con foco de teclado
/// y con toque; Escape lo cierra; se coloca dentro de la ventana y se asocia con aria-describedby.
const UI_SCRIPT: &str = r#"(function(){
var tip=document.createElement('div');tip.id='tip';tip.setAttribute('role','tooltip');tip.hidden=true;document.body.appendChild(tip);
var cur=null,canHover=matchMedia('(hover: hover)'),timer=null;
function place(){if(!cur)return;var r=cur.getBoundingClientRect(),w=tip.offsetWidth,h=tip.offsetHeight,m=8;
var x=Math.max(m,Math.min(r.left+r.width/2-w/2,innerWidth-w-m));var y=r.top-h-m;if(y<m)y=r.bottom+m;
tip.style.left=x+'px';tip.style.top=y+'px';}
function show(el){var t=el.getAttribute('data-tip');if(!t)return;if(cur&&cur!==el)cur.removeAttribute('aria-describedby');
cur=el;tip.textContent=t;tip.hidden=false;el.setAttribute('aria-describedby','tip');place();}
function hide(){if(cur){cur.removeAttribute('aria-describedby');cur=null;}tip.hidden=true;}
function tgt(e){return e.target.closest?e.target.closest('[data-tip]'):null;}
document.addEventListener('mouseover',function(e){var el=tgt(e);if(el&&canHover.matches)show(el);});
document.addEventListener('mouseout',function(e){var el=tgt(e);if(el&&(!e.relatedTarget||!el.contains(e.relatedTarget)))hide();});
document.addEventListener('focusin',function(e){var el=tgt(e);if(el&&el.matches(':focus-visible'))show(el);});
document.addEventListener('focusout',function(e){if(tgt(e))hide();});
document.addEventListener('keydown',function(e){
if(e.key==='Escape')hide();
if(e.key==='/'&&!e.ctrlKey&&!e.metaKey&&!e.altKey){var q=document.getElementById('q'),a=document.activeElement;
if(q&&a!==q&&!(a&&/^(INPUT|TEXTAREA|SELECT)$/.test(a.tagName)||a&&a.isContentEditable)){e.preventDefault();q.focus();q.select();}}});
document.addEventListener('click',function(e){
var c=e.target.closest?e.target.closest('[data-copy]'):null;
if(c){var v=c.getAttribute('data-copy'),orig=c.getAttribute('data-tip'),live=document.getElementById('live');
var done=function(ok){c.classList.toggle('copied',ok);c.setAttribute('data-tip',ok?'Kopierat!':'Kunde inte kopiera');if(live)live.textContent=ok?'Kopierat: '+v:'Kunde inte kopiera';show(c);
clearTimeout(timer);timer=setTimeout(function(){c.classList.remove('copied');c.setAttribute('data-tip',orig);if(cur===c)show(c);},1800);};
var legacy=function(){var t=document.createElement('textarea');t.value=v;t.setAttribute('readonly','');t.style.cssText='position:fixed;top:0;left:0;opacity:0';document.body.appendChild(t);t.select();var ok=false;try{ok=document.execCommand('copy');}catch(x){}document.body.removeChild(t);return ok;};
if(navigator.clipboard&&navigator.clipboard.writeText){navigator.clipboard.writeText(v).then(function(){done(true);},function(){done(legacy());});}else{done(legacy());}return;}
var el=tgt(e);
if(el&&el.tagName!=='A'&&!canHover.matches){(cur===el&&!tip.hidden)?hide():show(el);}else if(!el){hide();}});
addEventListener('scroll',hide,true);addEventListener('resize',hide);
})();"#;

/// Documento completo: `<html>` + navegación + `<main>`. `pathname` decide el enlace activo.
pub fn layout(title: &str, pathname: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="sv" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1, viewport-fit=cover";
                meta name="color-scheme" content="light dark";
                meta name="theme-color" content="#e9eff1" media="(prefers-color-scheme: light)";
                meta name="theme-color" content="#183340" media="(prefers-color-scheme: dark)";
                title { (title) }
                meta name="description" content=(DESCRIPTION);
                link rel="icon" href=(FAVICON);
                link rel="preconnect" href="https://fonts.googleapis.com";
                link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
                link rel="stylesheet" href=(FONTS_URL);
                link rel="stylesheet" href="/static/styles.css";
                script { (PreEscaped(HEAD_SCRIPT)) }
            }
            body {
                a.skip-link href="#main" { "Hoppa till innehållet" }
                div.shell {
                    (sidebar(pathname))
                    main #main.main tabindex="-1" { div.page { (content) } }
                }
                div #live.sr-only role="status" aria-live="polite" {}
                script { (PreEscaped(THEME_SCRIPT)) }
                script { (PreEscaped(UI_SCRIPT)) }
            }
        }
    }
}

/// `/foretag/<org>` se considera parte de "Sök företag".
fn nav_active(pathname: &str, href: &str) -> bool {
    pathname == href
        || pathname.starts_with(&format!("{href}/"))
        || (href == "/sok" && pathname.starts_with("/foretag"))
}

fn sidebar(pathname: &str) -> Markup {
    html! {
        aside.sidebar {
            div.side-head {
                a.brand href="/sok" aria-label="Siffra, till sökningen" { "Sif" span.accent { "f" } "ra" }
                button #theme-toggle.icon-btn type="button" aria-pressed="false" aria-label="Mörkt läge"
                    data-tip="Växla mellan ljust och mörkt läge" {
                    span.icon-moon { (icon("moon")) }
                    span.icon-sun { (icon("sun")) }
                }
            }
            nav.side-nav aria-label="Huvudmeny" {
                @for group in NAV.iter() {
                    div.nav-group {
                        div.nav-heading { (group.heading) }
                        @for item in group.items.iter() {
                            @let active = nav_active(pathname, item.href);
                            a.nav-link href=(item.href) aria-current=[active.then_some("page")] {
                                (icon(item.icon))
                                span.nav-text {
                                    span.label-long { (item.label) }
                                    span.label-short { (item.short) }
                                    @if let Some(badge) = item.badge {
                                        span.nav-badge { (badge) (sr(" nya")) }
                                    }
                                }
                            }
                        }
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

fn example_badge() -> Markup {
    html! { span.example-badge lang="es" data-tip="Fiktiva exempeldata, inte riktiga uppgifter." { "EJEMPLO" } }
}

fn risk_pill(level: RiskLevel) -> Markup {
    let label = match level {
        Severity::Good => "Låg risk",
        Severity::Warn => "Bevaka",
        Severity::Bad => "Förhöjd risk",
    };
    html! { span class={"pill pill-" (sev_class(level))} { span.pill-dot aria-hidden="true" {} (label) } }
}

fn status_pill(text: &str) -> Markup {
    html! { span.pill.pill-good { span.pill-dot aria-hidden="true" {} (text) } }
}

/// Una señal con icono propio por gravedad y texto para lectores de pantalla (no depende solo del color).
fn alert_row(severity: Severity, body: Markup) -> Markup {
    let (icon_name, label) = match severity {
        Severity::Good => ("check-circle", "Positiv signal: "),
        Severity::Warn => ("triangle-alert", "Varning: "),
        Severity::Bad => ("octagon-alert", "Allvarlig varning: "),
    };
    html! {
        li {
            span class={"alert-icon sev-" (sev_class(severity))} { (icon_sized(icon_name, "icon")) }
            span { (sr(label)) (body) }
        }
    }
}

/// `"1 234,5"`-estilo tkr → mkr con un decimal, para las etiquetas de las barras.
fn mkr(v: f64) -> String {
    format!("{:.1}", v / 1000.0)
}

fn benchmark_row(label: &str, term_tip: &str, pair: BenchmarkPair, scale_max: f64) -> Markup {
    let unit = "%";
    let value_pct = (pair.value / scale_max * 100.0).clamp(0.0, 100.0);
    let median_pct = (pair.median / scale_max * 100.0).clamp(0.0, 100.0);
    let aria = format!("{label}: företaget {} {unit}, branschmedian {} {unit}", pair.value, pair.median);
    let diff = pair.value - pair.median;
    let tip = format!(
        "{label}: företaget {} {unit}, branschmedian {} {unit}. {} medianen med {:.1} procentenheter.",
        pair.value,
        pair.median,
        if diff >= 0.0 { "Över" } else { "Under" },
        diff.abs()
    );
    html! {
        div.bench {
            div.bench-top {
                span.bench-label { (term(label, term_tip)) }
                span.bench-val {
                    span.neg[pair.value < 0.0] { (pair.value) " " (unit) }
                    span.bench-median-text { " · median " (pair.median) " " (unit) }
                }
            }
            div.bench-track role="img" tabindex="0" aria-label=(aria) data-tip=(tip) {
                div.bench-fill style=(format!("width:{}%", value_pct)) {}
                span.bench-marker style=(format!("left:{}%", median_pct)) {}
            }
        }
    }
}

fn bench_legend() -> Markup {
    html! {
        div.legend aria-hidden="true" {
            span.legend-item { span.swatch.fill {} "Företaget" }
            span.legend-item { span.swatch.marker {} "Branschmedian" }
        }
    }
}

/// Barras de comparación + leyenda + nota de fuente. Se sirve en la ficha o como fragmento diferido.
/// El valor de la empresa es de EJEMPLO; la mediana es real si SCB respondió.
pub fn benchmark_fragment(company: &Company, medians: Option<&SectorMedians>, notice: Option<&str>) -> Markup {
    let sni_code: String = company.sni.chars().take(6).collect();
    let b = &company.benchmarks;
    let with_median = |pair: BenchmarkPair, real: Option<f64>| BenchmarkPair {
        median: real.unwrap_or(pair.median),
        ..pair
    };
    let margin = with_median(b.margin, medians.map(|m| m.margin));
    let solidity = with_median(b.solidity, medians.map(|m| m.solidity));
    let liquidity = with_median(b.liquidity, medians.map(|m| m.liquidity));
    html! {
        (benchmark_row("Vinstmarginal", TIP_MARGIN, margin, 30.0))
        (benchmark_row("Soliditet", TIP_SOLIDITET_BENCH, solidity, 80.0))
        (benchmark_row("Kassalikviditet", TIP_LIQUIDITY, liquidity, 250.0))
        (bench_legend())
        @if let Some(n) = notice {
            p.notice role="status" { (n) }
        }
        @if let Some(m) = medians {
            p.note {
                "Strecket visar medianen för SNI " (m.sni_code)
                @if !m.sni_label.is_empty() { " (" (m.sni_label) ")" }
                ", "
                @if m.size_class == "TOT" {
                    "alla storleksklasser"
                } @else {
                    (m.size_class.replace("001", "0")) " anställda"
                }
                ", " (m.year) ". Källa: SCB, branschnyckeltal."
                @if !m.exact_sni || !m.exact_size {
                    " SCB saknar data för " (sni_code) ", " (company.employee_range)
                    " anställda — närmaste nivå visas."
                }
                " Företagets egna värden är " (example_badge()) "."
            }
        } @else {
            p.note { "Strecket visar medianen för SNI " (sni_code) ". " (example_badge()) }
        }
    }
}

/// Gráfico de barras SVG de la omsättning de 5 años, con etiqueta de valor en cada barra.
fn revenue_chart(revenue: &FiveYearSeries) -> Markup {
    let (w, h) = (360.0_f64, 230.0_f64);
    let (pl, pb, pt, pr) = (50.0_f64, 28.0_f64, 22.0_f64, 6.0_f64);
    let max = *revenue.iter().max().unwrap_or(&0) as f64;
    let top = {
        let t = (max / 20000.0).ceil() * 20000.0;
        if t == 0.0 { 20000.0 } else { t }
    };
    let bw = (w - pl - pr) / 5.0;

    let grid_lines: Vec<(f64, f64)> = (0..5)
        .map(|i| {
            let value = top / 4.0 * i as f64;
            let y = h - pb - (h - pb - pt) * value / top;
            (value, y)
        })
        .collect();

    let desc = revenue
        .iter()
        .enumerate()
        .map(|(i, v)| format!("{}: {} tkr", FINANCIAL_YEARS[i], format_tkr(*v)))
        .collect::<Vec<_>>()
        .join(", ");

    html! {
        svg.chart viewBox=(format!("0 0 {} {}", w, h)) role="group" aria-labelledby="rc-title rc-desc" {
            title #rc-title { "Omsättning, fem år (tkr)" }
            desc #rc-desc { (desc) }
            @for (value, y) in grid_lines.iter() {
                g {
                    line x1=(pl) x2=(w - pr) y1=(y) y2=(y) stroke="var(--line)" stroke-width="1" {}
                    text x=(pl - 8.0) y=(y + 4.0) text-anchor="end" { (value / 1000.0) " mkr" }
                }
            }
            @for (i, v) in revenue.iter().enumerate() {
                @let v = *v as f64;
                @let bar_h = (h - pb - pt) * v / top;
                @let x = pl + i as f64 * bw + bw * 0.18;
                @let bar_w = bw * 0.64;
                @let y = h - pb - bar_h;
                @let is_last = i == revenue.len() - 1;
                @let change = if i > 0 { Some((v / revenue[i - 1] as f64 - 1.0) * 100.0) } else { None };
                @let tip = match change {
                    Some(c) => format!("{}: {} tkr · {}{:.1} % mot {}", FINANCIAL_YEARS[i], format_tkr(v as i64),
                        if c >= 0.0 { "+" } else { "−" }, c.abs(), FINANCIAL_YEARS[i - 1]),
                    None => format!("{}: {} tkr", FINANCIAL_YEARS[i], format_tkr(v as i64)),
                };
                g.bar-group tabindex="0" role="img" aria-label=(tip) data-tip=(tip) {
                    rect.hit x=(pl + i as f64 * bw) y=(pt) width=(bw) height=(h - pb - pt) fill="transparent" {}
                    rect.bar x=(x) y=(y) width=(bar_w) height=(bar_h) rx="2"
                        fill=(if is_last { "var(--bar)" } else { "var(--bar2)" }) {}
                    text x=(x + bar_w / 2.0) y=(h - 9.0) text-anchor="middle" { (FINANCIAL_YEARS[i]) }
                    text.value-label.strong[is_last] x=(x + bar_w / 2.0) y=(y - 6.0) text-anchor="middle" {
                        (mkr(v))
                    }
                }
            }
        }
    }
}

/// Gráfico de barras SVG de la caja proyectada, a partir del saldo inicial y las entradas/salidas semanales.
fn cash_flow_chart(weeks: &[CashWeek], start_balance: i64) -> Markup {
    const THRESHOLD: i64 = 150; // por debajo de este saldo la barra se marca como riesgo
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
    let (pl, pb, pt) = (48.0_f64, 28.0_f64, 26.0_f64);
    let lo = (min.min(0)) as f64;
    let hi = (max as f64 / 100.0).ceil() * 100.0;
    let bw = (w - pl - 8.0) / weeks.len() as f64;

    let y = |v: f64| h - pb - (h - pb - pt) * (v - lo) / (hi - lo);
    let grid_values = [lo, js_round((lo + hi) / 2.0 / 100.0) * 100.0, hi];
    let lowest_idx = points.iter().position(|&p| p == min).unwrap_or(0);
    let desc = weeks
        .iter()
        .zip(&points)
        .map(|(wk, p)| format!("vecka {}: {} tkr", wk.week, p))
        .collect::<Vec<_>>()
        .join(", ");

    html! {
        figure {
            div.chart-scroll {
                svg.chart viewBox=(format!("0 0 {} {}", w, h)) role="group" aria-labelledby="cc-title cc-desc" {
                    title #cc-title { "Prognos för kassa, 13 veckor (tkr)" }
                    desc #cc-desc { "Förväntad kassa per vecka. " (desc) }
                    defs {
                        pattern #hatch width="6" height="6" patternUnits="userSpaceOnUse" patternTransform="rotate(45)" {
                            rect width="6" height="6" fill="var(--bad)" {}
                            rect width="2" height="6" fill="var(--surface)" {}
                        }
                    }
                    text x="0" y="12" { "tkr" }
                    @for v in grid_values.iter() {
                        g {
                            line x1=(pl) x2=(w - 8.0) y1=(y(*v)) y2=(y(*v)) stroke="var(--line)" {}
                            text x=(pl - 8.0) y=(y(*v) + 4.0) text-anchor="end" { (v) }
                        }
                    }
                    @for (i, v) in points.iter().enumerate() {
                        @let v = *v;
                        @let x = pl + i as f64 * bw + bw * 0.15;
                        @let bar_w = bw * 0.7;
                        @let y0 = y(0.0);
                        @let y1 = y(v as f64);
                        @let net = weeks[i].inflow - weeks[i].outflow;
                        @let tip = format!("Vecka {}: kassa {} tkr · in {} · ut {} · netto {}{}",
                            weeks[i].week, v, weeks[i].inflow, weeks[i].outflow, format_tkr(net),
                            if v < THRESHOLD { " · under gränsen" } else { "" });
                        g.bar-group tabindex="0" role="img" aria-label=(tip) data-tip=(tip) {
                            rect.hit x=(pl + i as f64 * bw) y=(pt) width=(bw) height=(h - pb - pt) fill="transparent" {}
                            rect.bar x=(x) y=(y0.min(y1)) width=(bar_w) height=((y1 - y0).abs()) rx="2"
                                fill=(if v < THRESHOLD { "url(#hatch)" } else { "var(--bar)" })
                                stroke=(if v < THRESHOLD { "var(--bad)" } else { "none" }) {}
                            text x=(x + bar_w / 2.0) y=(h - 9.0) text-anchor="middle" { "v" (weeks[i].week) }
                            @if i == lowest_idx {
                                text.value-label.strong x=(x + bar_w / 2.0) y=(y1 - 6.0) text-anchor="middle" { (v) }
                            }
                        }
                    }
                    g.threshold-group tabindex="0" role="img" aria-label="Gräns 150 tkr: staplar under gränsen markeras"
                        data-tip="Gräns 150 tkr: staplar med lägre kassa markeras som risk (exempelvärde)." {
                        rect x=(w - 96.0) y=(y(THRESHOLD as f64) - 20.0) width="88" height="20" fill="transparent" {}
                        line.threshold x1=(pl) x2=(w - 8.0) y1=(y(THRESHOLD as f64)) y2=(y(THRESHOLD as f64)) {}
                        text.threshold-label x=(w - 10.0) y=(y(THRESHOLD as f64) - 5.0) text-anchor="end" { "gräns " (THRESHOLD) }
                    }
                }
            }
            p.scroll-hint { "Svep i sidled för att se alla veckor." }
            div.legend {
                span.legend-item { span.swatch.fill {} "Kassa" }
                span.legend-item { span.swatch.hatch {} "Under gränsen" }
                span.legend-item { span.swatch.dash {} "Gräns " (THRESHOLD) " tkr" }
            }
            figcaption.chart-caption {
                "Kassan kommer nära " strong { (min) " tkr" } " i vecka " (weeks[lowest_idx].week) "."
            }
        }
        details.chart-data {
            summary { "Visa värden som tabell" }
            div.table-wrap {
                table {
                    caption.sr-only { "Förväntad kassa per vecka, tkr" }
                    thead { tr { th scope="col" { "Vecka" } th.right scope="col" { "In" } th.right scope="col" { "Ut" } th.right scope="col" { "Kassa" } } }
                    tbody {
                        @for (wk, p) in weeks.iter().zip(&points) {
                            tr {
                                th scope="row" { (wk.week) }
                                td.right.num { (wk.inflow) }
                                td.right.num { (wk.outflow) }
                                td.right.num { (p) }
                            }
                        }
                    }
                }
            }
        }
    }
}

// ───────────────────────── Páginas ─────────────────────────

fn urlencode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        match b {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(b as char),
            _ => out.push_str(&format!("%{b:02X}")),
        }
    }
    out
}

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

fn sortable_th(label: &str, col: &str, q: &str, sort: Option<&str>, dir: &str, right: bool, hide_sm: bool) -> Markup {
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
    let what = match col {
        "name" => "företag",
        "revenue" => "omsättning",
        _ => "resultat",
    };
    let tip = format!("Sortera {} efter {what}", if next == "asc" { "stigande" } else { "fallande" });
    html! {
        th.right[right].hide-sm[hide_sm] scope="col" aria-sort=[aria_sort] {
            a.sort-link href=(sok_url(q, Some((col, next)))) data-tip=(tip) {
                (label) (icon_sized(icon_name, "icon-sm"))
            }
        }
    }
}

pub fn sok_page(query: &str, sort: Option<&str>, dir: Option<&str>, live: Option<&Organisation>) -> Markup {
    let mut results = search_example_companies(query);
    let live = live.filter(|_| results.is_empty());
    let sort = sort.filter(|s| ["name", "revenue", "result"].contains(s));
    let dir = match dir {
        Some("asc") => "asc",
        Some("desc") => "desc",
        _ => sort.map(default_dir).unwrap_or("asc"),
    };
    match sort {
        Some("name") => results.sort_by_key(|c| c.name.to_lowercase()),
        Some("revenue") => results.sort_by_key(|c| c.financials.revenue[4]),
        Some("result") => results.sort_by_key(|c| c.financials.result[4]),
        _ => {}
    }
    if sort.is_some() && dir == "desc" {
        results.reverse();
    }

    let total = results.len() + usize::from(live.is_some());
    let count_text = if query.trim().is_empty() {
        format!("{total} företag")
    } else {
        format!("{total} {} för ”{}”", if total == 1 { "träff" } else { "träffar" }, query.trim())
    };

    layout(
        "Sök företag — Siffra",
        "/sok",
        html! {
            div.page-head {
                h1.page-title { "Sök företag" }
                p.lead lang="es" {
                    "Interfaz de producto en sueco. Estas tres empresas son un " strong { "EJEMPLO" }
                    " ficticio; en producción esta lista vendrá de SCB y Bolagsverket."
                }
            }
            form.search role="search" method="get" action="/sok" {
                label.field-label for="q" { "Namn, organisationsnummer eller ort" }
                div.search-row {
                    div.search-field {
                        (icon("search"))
                        input #q.search-input type="search" name="q" value=(query)
                            placeholder="t.ex. Nordlys, 559012-3456 eller Göteborg"
                            autocomplete="off" enterkeyhint="search";
                    }
                    button.btn.btn-primary type="submit" { "Sök" }
                    @if !query.is_empty() {
                        a.btn href="/sok" { "Rensa" }
                    }
                }
                p.field-hint.js-only { "Tryck " kbd { "/" } " för att hoppa till sökfältet." }
            }
            p.result-count role="status" { (count_text) }
            div.table-wrap {
                table.row-link {
                    caption.sr-only { "Företag" }
                    thead {
                        tr {
                            (sortable_th("Företag", "name", query, sort, dir, false, false))
                            th.hide-sm scope="col" { "Ort" }
                            (sortable_th("Omsättning 2024 (tkr)", "revenue", query, sort, dir, true, false))
                            (sortable_th("Resultat (tkr)", "result", query, sort, dir, true, true))
                            th scope="col" { "Risk" }
                        }
                    }
                    tbody {
                        @for c in results.iter() {
                            tr {
                                td.top {
                                    a.company-link href=(format!("/foretag/{}", c.org_number)) { (c.name) }
                                    div.org-sub { (c.org_number) span.city-sub { (c.city) } }
                                }
                                td.top.hide-sm { (c.city) }
                                td.top.right.num.mono { (format_tkr(c.financials.revenue[4])) }
                                td.top.right.num.mono.hide-sm.neg[c.financials.result[4] < 0] { (format_tkr(c.financials.result[4])) }
                                td.top { (risk_pill(c.risk_level)) }
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
                        @if results.is_empty() && live.is_none() {
                            tr {
                                td colspan="5" {
                                    div.empty-state {
                                        strong { "Inga träffar" }
                                        p { "Prova ett annat namn eller organisationsnummer, eller sök på en ort." }
                                        div.chips {
                                            a.chip href=(sok_url("Göteborg", None)) { "Göteborg" }
                                            a.chip href=(sok_url("Uppsala", None)) { "Uppsala" }
                                            a.chip href=(sok_url("559108-7721", None)) { "559108-7721" }
                                            a.chip href="/sok" { "Visa alla" }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
            }
            @if live.is_some() {
                p.note { "Resultat från Bolagsverket (gratis API, sökning på organisationsnummer)." }
            } @else {
                p.note lang="es" { (example_badge()) " Tres empresas ficticias. En producción esta lista viene de SCB y Bolagsverket." }
            }
            // Filtrado "en vivo" como el original: reenvía el formulario al escribir (con retardo).
            script { (PreEscaped(SEARCH_SCRIPT)) }
        },
    )
}

const SEARCH_SCRIPT: &str = r#"(function(){var i=document.getElementById('q');if(!i)return;
if(i.value&&location.search.indexOf('q=')>-1&&!document.querySelector('[aria-sort]')){i.focus();var n=i.value.length;try{i.setSelectionRange(n,n);}catch(e){}}
var t;i.addEventListener('input',function(){clearTimeout(t);t=setTimeout(function(){i.form.submit();},350);});})();"#;

/// Estado de la tarjeta de comparación con el sector al renderizar la ficha.
pub enum Bench {
    /// SCB desactivado: medianas de EJEMPLO.
    Example,
    /// Resultado de SCB ya en caché (o "sin datos").
    Ready(Option<SectorMedians>),
    /// Hay que preguntar a SCB: esqueleto de carga y fragmento diferido.
    Pending,
}

const BENCH_SCRIPT: &str = r#"(function(){var c=document.getElementById('bench-body');if(!c)return;var u=c.getAttribute('data-fragment');
function fail(){c.removeAttribute('aria-busy');var s=c.querySelectorAll('.skeleton-row');for(var i=0;i<s.length;i++)s[i].hidden=true;var e=c.querySelector('.bench-error');if(e)e.hidden=false;}
fetch(u,{headers:{'Accept':'text/html'}}).then(function(r){if(!r.ok)throw 0;return r.text();}).then(function(h){c.innerHTML=h;c.removeAttribute('aria-busy');}).catch(fail);})();"#;

pub fn company_page(company: &Company, tab: &str, bench: &Bench) -> Markup {
    let f = &company.financials;
    let growth = (f.revenue[4] as f64 / f.revenue[3] as f64 - 1.0) * 100.0;
    let margin = f.result[4] as f64 / f.revenue[4] as f64 * 100.0;
    let solidity = f.equity[4] as f64 / f.total_assets[4] as f64 * 100.0;

    layout(
        DEFAULT_TITLE,
        &format!("/foretag/{}", company.org_number),
        html! {
            a.back href="/sok" { (icon_sized("arrow-left", "icon-sm")) "Sökresultat" }

            div.company-head {
                div {
                    h1.company-name { (company.name) }
                    div.company-meta {
                        (term("Org.nr", "Organisationsnummer: bolagets unika nummer hos Bolagsverket.")) " " (company.org_number)
                        button.copy-btn type="button" data-copy=(company.org_number) data-tip="Kopiera organisationsnummer"
                            aria-label=(format!("Kopiera organisationsnummer {}", company.org_number)) {
                            span.icon-copy { (icon_sized("copy", "icon-sm")) }
                            span.icon-check { (icon_sized("check", "icon-sm")) }
                        }
                        " · " (company.legal_form) " · " (company.city)
                    }
                }
                div.pills {
                    (status_pill(company.status))
                    (risk_pill(company.risk_level))
                }
            }

            dl.kpis {
                (kpi("Omsättning", "Nettoomsättning: bolagets försäljning under året, i tusen kronor (tkr).",
                    &format!("{} tkr", format_tkr(f.revenue[4])), false,
                    html! {
                        (icon_sized(if growth >= 0.0 { "arrow-up-right" } else { "arrow-down-right" }, "icon-sm"))
                        (format!("{:.1} % mot 2023", growth.abs()))
                    },
                    if growth >= 0.0 { "up" } else { "down" }))
                (kpi("Resultat", "Resultat efter finansiella poster, i tusen kronor (tkr).",
                    &format!("{} tkr", format_tkr(f.result[4])), f.result[4] < 0,
                    html! { (format!("Vinstmarginal {:.1} %", margin)) }, ""))
                (kpi("Soliditet", TIP_SOLIDITET, &format!("{:.1} %", solidity), false,
                    html! { (format!("Eget kapital {} tkr", format_tkr(f.equity[4]))) }, ""))
                (kpi("Anställda", "Antal anställda enligt SCB, angivet som intervall.",
                    company.employee_range, false, html! { "SCB, intervall" }, ""))
            }

            (company_tabs(company, tab, bench))
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

const SUB_TABS: [(&str, &str); 4] = [
    ("ov", "Översikt"),
    ("fin", "Bokslut"),
    ("ppl", "Personer"),
    ("ai", "Sammanfattning"),
];

fn company_tabs(company: &Company, tab: &str, bench: &Bench) -> Markup {
    // Pestaña desconocida → Översikt (la inicial del original).
    let active = SUB_TABS.iter().find(|(id, _)| *id == tab).map(|(id, _)| *id).unwrap_or("ov");
    html! {
        nav.tabs #vyer aria-label="Företagsvyer" {
            @for (id, label) in SUB_TABS.iter() {
                a.tab aria-current=[(*id == active).then_some("page")]
                    href=(format!("/foretag/{}?tab={}#vyer", company.org_number, id)) { (label) }
            }
        }
        @match active {
            "fin" => { (financials(company)) }
            "ppl" => { (people(company)) }
            "ai" => { (summary(company)) }
            _ => { (overview(company, bench)) }
        }
    }
}

fn overview(company: &Company, bench: &Bench) -> Markup {
    let revenue = &company.financials.revenue;
    html! {
        div.overview-grid {
            div.card {
                h2.card-title {
                    "Omsättning, 5 år (tkr)"
                    (info_btn("Om diagrammet", "tkr = tusen kronor, mkr = miljoner kronor. Håll muspekaren över, tabba till eller tryck på en stapel för exakta värden."))
                }
                (revenue_chart(revenue))
                div.legend aria-hidden="true" {
                    span.legend-item { span.swatch.past {} "Tidigare år" }
                    span.legend-item { span.swatch.fill {} "Senaste året" }
                    span.legend-item { "Värden i mkr" }
                }
                details.chart-data {
                    summary { "Visa värden som tabell" }
                    div.table-wrap {
                        table {
                            caption.sr-only { "Omsättning per år, tkr" }
                            thead { tr { th scope="col" { "År" } th.right scope="col" { "tkr" } } }
                            tbody {
                                @for (y, v) in FINANCIAL_YEARS.iter().zip(revenue.iter()) {
                                    tr { th scope="row" { (y) } td.right.num.mono { (format_tkr(*v)) } }
                                }
                            }
                        }
                    }
                }
            }
            div.card {
                h2.card-title {
                    "Mot branschen (SNI-median)"
                    (info_btn("Om branschjämförelsen", "SNI är branschkoden från Statistiska centralbyrån. Medianen är mittvärdet: hälften av företagen ligger över och hälften under. Strecket i varje stapel markerar medianen."))
                }
                @match bench {
                    Bench::Pending => {
                        div #bench-body aria-busy="true" data-fragment=(format!("/foretag/{}/benchmarks", company.org_number)) {
                            (sr("Hämtar branschmedianer från SCB…"))
                            @for _ in 0..3 {
                                div.skeleton-row aria-hidden="true" { div.skeleton-line {} div.skeleton-bar {} }
                            }
                            p.bench-error hidden { "Kunde inte hämta branschmedianer från SCB just nu. Ladda om sidan för att försöka igen." }
                            noscript { (benchmark_fragment(company, None, Some("JavaScript krävs för att hämta SCB-data. Visar exempelvärden."))) }
                        }
                        script { (PreEscaped(BENCH_SCRIPT)) }
                    }
                    Bench::Ready(m) => { div #bench-body { (benchmark_fragment(company, m.as_ref(), None)) } }
                    Bench::Example => { div #bench-body { (benchmark_fragment(company, None, None)) } }
                }
            }
        }

        div.card.mt-4 {
            h2.card-title { "Signaler" }
            ul.alert-list {
                @for a in company.alerts.iter() {
                    (alert_row(a.severity, html! { (a.text) }))
                }
            }
        }
        p.note { "Verksamhet: " (company.sni) }
    }
}

fn financials(company: &Company) -> Markup {
    let f = &company.financials;
    let rows: [(&str, &[i64; 5]); 4] = [
        ("Omsättning", &f.revenue),
        ("Resultat efter finansiella poster", &f.result),
        ("Eget kapital", &f.equity),
        ("Summa tillgångar", &f.total_assets),
    ];
    html! {
        div.table-wrap {
            table.fin-table {
                caption.sr-only { "Bokslut, fem år, tkr" }
                thead {
                    tr {
                        th scope="col" { "tkr" }
                        @for y in FINANCIAL_YEARS.iter() { th.right scope="col" { (y) } }
                    }
                }
                tbody {
                    @for (label, values) in rows.iter() {
                        tr {
                            th scope="row" { (label) }
                            @for v in values.iter() {
                                td.right.num.mono.neg[*v < 0] { (format_tkr(*v)) }
                            }
                        }
                    }
                }
            }
        }
        p.note { "Källa: digitalt inlämnade årsredovisningar (iXBRL), Bolagsverket. " (example_badge()) }
        div.mt-3 {
            button.btn type="button" disabled title="Inte kopplat till Bolagsverket ännu" { "Ladda ner årsredovisning (zip)" }
        }
    }
}

fn people(company: &Company) -> Markup {
    html! {
        div.table-wrap {
            table {
                caption.sr-only { "Personer och roller" }
                thead { tr { th scope="col" { "Roll" } th scope="col" { "Namn" } } }
                tbody {
                    @for p in company.people.iter() {
                        tr { td { (p.role) } td { (p.name) } }
                    }
                }
            }
        }
        p.mt-3 { "Firmateckning: " strong { (company.firmateckning) } }
        p.note { "Kräver Bolagsverkets betalda API (fas 2). Namn är påhittade. " (example_badge()) }
    }
}

fn summary(company: &Company) -> Markup {
    html! {
        div.card {
            h2.card-title { "Automatisk sammanfattning" }
            p.summary-text { (generate_example_summary(company)) }
            p.note { "Genererad från siffrorna på fliken Bokslut. Ingen kreditbedömning. " (example_badge()) }
        }
    }
}

/// Ficha con datos REALES de Bolagsverket (API gratuito). Solo hay datos básicos: las cifras
/// financieras, personas y riesgo requieren otras fuentes y todavía no están conectadas.
pub fn live_profile_page(o: &Organisation) -> Markup {
    let address = [o.gatuadress.clone(), Some([o.postnummer.clone(), o.postort.clone()].into_iter().flatten().collect::<Vec<_>>().join(" "))]
        .into_iter()
        .flatten()
        .filter(|t| !t.is_empty())
        .collect::<Vec<_>>()
        .join(", ");
    layout(
        DEFAULT_TITLE,
        &format!("/foretag/{}", o.organisationsnummer),
        html! {
            a.back href="/sok" { (icon_sized("arrow-left", "icon-sm")) "Sökresultat" }

            div.company-head {
                div {
                    h1.company-name { (o.namn) }
                    div.company-meta {
                        (term("Org.nr", "Organisationsnummer: bolagets unika nummer hos Bolagsverket.")) " " (o.formatted_number())
                        button.copy-btn type="button" data-copy=(o.formatted_number()) data-tip="Kopiera organisationsnummer"
                            aria-label=(format!("Kopiera organisationsnummer {}", o.formatted_number())) {
                            span.icon-copy { (icon_sized("copy", "icon-sm")) }
                            span.icon-check { (icon_sized("check", "icon-sm")) }
                        }
                        " · " (o.organisationsform)
                        @if let Some(ort) = &o.postort { " · " (ort) }
                    }
                }
                div.pills {
                    @if o.aktiv {
                        (status_pill("Aktiv"))
                    } @else {
                        span.pill.pill-bad {
                            span.pill-dot aria-hidden="true" {}
                            @if let Some(d) = &o.avregistreringsdatum { "Avregistrerad " (d) } @else { "Inaktiv" }
                        }
                    }
                    @for f in o.forfaranden.iter() {
                        span.pill.pill-bad { span.pill-dot aria-hidden="true" {} (f) }
                    }
                }
            }

            div.card {
                h2.card-title { "Företagsuppgifter" }
                dl.facts {
                    dt { "Postadress" } dd { @if address.is_empty() { "—" } @else { (address) } }
                    dt { "Registrerad" } dd { @if o.registreringsdatum.is_empty() { "—" } @else { (o.registreringsdatum) } }
                    dt { (term("Bransch (SNI)", "SNI är branschkoden från Statistiska centralbyrån.")) }
                    dd {
                        @if o.sni.is_empty() { "—" } @else {
                            @for (i, (kod, text)) in o.sni.iter().enumerate() {
                                @if i > 0 { " · " }
                                span.mono { (kod) } @if !text.is_empty() { " " (text) }
                            }
                        }
                    }
                    dt { "Verksamhet" } dd { (o.verksamhetsbeskrivning.clone().unwrap_or_else(|| "—".to_string())) }
                }
            }
            p.note {
                "Källa: Bolagsverket, värdefulla datamängder (live). Omsättning, resultat, personer och riskbedömning visas inte här — de kräver årsredovisningar och andra källor som ännu inte är kopplade."
            }
        },
    )
}

pub fn live_error_page() -> Markup {
    layout(
        "Kunde inte hämta uppgifterna — Siffra",
        "/foretag",
        html! {
            div.page-head {
                h1.page-title { "Kunde inte hämta uppgifterna" }
                p.lead { "Bolagsverkets API svarade inte som väntat. Försök igen om en stund, eller kontrollera loggen på servern." }
            }
            a.btn.btn-primary href="/sok" { (icon_sized("search", "icon-sm")) "Till sökningen" }
        },
    )
}

pub fn company_not_found_page() -> Markup {
    layout(
        DEFAULT_TITLE,
        "/foretag",
        html! {
            div.page-head {
                h1.page-title { "Företaget hittades inte" }
                p.lead {
                    "I det här scaffoldet finns bara de tre EJEMPLO-företagen. Sök på namn, organisationsnummer eller ort för att hitta dem."
                }
            }
            a.btn.btn-primary href="/sok" { (icon_sized("search", "icon-sm")) "Till sökningen" }
        },
    )
}

pub fn not_found_page() -> Markup {
    layout(
        "404 — Siffra",
        "",
        html! {
            div.page-head {
                h1.page-title { "404 — Sidan hittades inte" }
                p.lead { "Sidan du letar efter finns inte eller har flyttats." }
            }
            a.btn.btn-primary href="/sok" { (icon_sized("search", "icon-sm")) "Till sökningen" }
        },
    )
}

pub fn bevakning_page() -> Markup {
    layout(
        "Bevakning — Siffra",
        "/bevakning",
        html! {
            div.page-head { h1.page-title { "Bevakning" } }
            div.card.narrow {
                ul.alert-list {
                    @for a in EXAMPLE_WATCH_ALERTS.iter() {
                        (alert_row(a.severity, html! {
                            @if let Some(c) = EXAMPLE_COMPANIES.iter().find(|c| c.name == a.company_name) {
                                a.alert-link href=(format!("/foretag/{}", c.org_number)) { (a.company_name) }
                            } @else {
                                strong { (a.company_name) }
                            }
                            br;
                            (a.text) " " span.muted { "· " (a.when) }
                        }))
                    }
                }
            }
            p.note lang="es" {
                (example_badge())
                " Mail varje måndag med det som ändrats. Fase 2 del plan de trabajo — depende de notificaciones de Bolagsverket (API de pago) o comparación semanal del archivo gratuito."
            }
        },
    )
}

pub fn likviditet_page() -> Markup {
    layout(
        "Likviditetsprognos — Siffra",
        "/likviditet",
        html! {
            div.page-head { h1.page-title { "Likviditetsprognos, 13 veckor" } }
            div.card.narrow {
                h2.card-title {
                    "Förväntad kassa (tkr)"
                    (info_btn("Om diagrammet", "Kassa per vecka: startsaldot plus inbetalningar minus utbetalningar. Håll muspekaren över, tabba till eller tryck på en stapel för detaljer."))
                }
                (cash_flow_chart(&EXAMPLE_CASH_WEEKS, EXAMPLE_CASH_START_BALANCE))
            }
            div.card.narrow.mt-3 {
                ul.alert-list {
                    (alert_row(Severity::Warn, html! {
                        span lang="es" { "Momsinbetalning och löner infaller samma vecka som el mínimo de caja." }
                    }))
                }
            }
            p.note lang="es" {
                (example_badge())
                " Beräknat från kund- och leverantörsfakturor och återkommande betalningar. Fase 3 del plan de trabajo — requiere importación SIE."
            }
        },
    )
}

pub fn sie_page() -> Markup {
    layout(
        "Importera SIE — Siffra",
        "/sie",
        html! {
            div.page-head { h1.page-title { "Importera bokföring (SIE)" } }
            div.dropzone aria-disabled="true" {
                (icon("upload"))
                strong { "Släpp en SIE-fil här" }
                p { "SIE 4 från Fortnox, Visma, Bokio eller annat program." }
                button.btn type="button" disabled { "Välj fil" }
                p.fine { "Ej aktiv i demon: filimport är inte kopplad ännu." }
            }
            div.card.narrow.mt-3 {
                h2.card-title { "Förhandsvisning" }
                div.scroll-x {
                    table {
                        caption.sr-only { "Förhandsvisning av konton" }
                        thead {
                            tr {
                                th scope="col" { "Konto (BAS)" }
                                th scope="col" { "Namn" }
                                th.right scope="col" { "Saldo" }
                            }
                        }
                        tbody {
                            @for row in EXAMPLE_SIE_PREVIEW.iter() {
                                tr {
                                    td.mono { (row.account) }
                                    td { (row.name) }
                                    td.right.num.mono.neg[row.balance_sek < 0] { (format_int(row.balance_sek)) }
                                }
                            }
                        }
                    }
                }
            }
            p.note lang="es" {
                (example_badge())
                " No hay parser SIE real todavía (Fase 3). Este es solo el layout de la pantalla."
            }
        },
    )
}

pub fn fakturor_page() -> Markup {
    let sum = |status: &str| -> i64 {
        EXAMPLE_INVOICES.iter().filter(|i| i.status == status).map(|i| i.amount_sek).sum()
    };
    layout(
        "Kundfakturor — Siffra",
        "/fakturor",
        html! {
            div.page-head { h1.page-title { "Kundfakturor" } }
            dl.stats {
                div.stat { dt { "Förfallet" } dd.neg { (format_int(sum("Förfallen"))) " kr" } }
                div.stat { dt { "Obetalt" } dd { (format_int(sum("Obetald"))) " kr" } }
                div.stat { dt { "Betalt" } dd { (format_int(sum("Betald"))) " kr" } }
            }
            div.table-wrap {
                table {
                    caption.sr-only { "Kundfakturor" }
                    thead {
                        tr {
                            th scope="col" { "Nr" }
                            th scope="col" { "Kund" }
                            th.right scope="col" { "Belopp (kr)" }
                            th scope="col" { "Förfaller" }
                            th scope="col" { "Status" }
                        }
                    }
                    tbody {
                        @for inv in EXAMPLE_INVOICES.iter() {
                            tr {
                                td.mono { (inv.number) }
                                td { (inv.customer) }
                                td.right.num.mono { (format_int(inv.amount_sek)) }
                                td.num { (inv.due_date) }
                                td { span class={"pill pill-" (sev_class(inv.severity))} { span.pill-dot aria-hidden="true" {} (inv.status) } }
                            }
                        }
                    }
                }
            }
            p.note lang="es" {
                (example_badge())
                " Fase 3+ del plan de trabajo. El cliente podrá ser avisado si el pagador tiene riesgo elevado."
            }
        },
    )
}
