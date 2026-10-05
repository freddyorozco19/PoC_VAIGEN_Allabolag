//! Layout, componentes y páginas. Equivale a `src/components/*` y `src/app/**/page.tsx`.

use maud::{html, Markup, DOCTYPE};

use crate::format::{format_int, format_tkr, js_round};
use crate::model::*;
use crate::summary::generate_example_summary;

// ───────────────────────── Layout ─────────────────────────

struct NavItem {
    href: &'static str,
    label: &'static str,
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
            NavItem { href: "/sok", label: "Sök företag", badge: None },
            NavItem { href: "/bevakning", label: "Bevakning", badge: Some("3") },
        ],
    },
    NavGroup {
        heading: "Min verksamhet",
        items: &[
            NavItem { href: "/likviditet", label: "Likviditetsprognos", badge: None },
            NavItem { href: "/sie", label: "Importera SIE", badge: None },
            NavItem { href: "/fakturor", label: "Fakturor", badge: None },
        ],
    },
];

const DEFAULT_TITLE: &str = "Siffra — MVP";
const DESCRIPTION: &str = "Siffra (nombre de trabajo): inteligencia financiera de empresas para el mercado sueco. Proyecto en construcción — ver README para el estado real.";
const FONTS_URL: &str = "https://fonts.googleapis.com/css2?family=Bricolage+Grotesque:wght@500;700&family=IBM+Plex+Mono:wght@400;500&family=IBM+Plex+Sans:wght@400;500;600&display=swap";

/// Documento completo: `<html>` + sidebar + `<main>`. `pathname` decide el enlace activo del sidebar.
pub fn layout(title: &str, pathname: &str, content: Markup) -> Markup {
    html! {
        (DOCTYPE)
        html lang="sv" {
            head {
                meta charset="utf-8";
                meta name="viewport" content="width=device-width, initial-scale=1";
                title { (title) }
                meta name="description" content=(DESCRIPTION);
                link rel="preconnect" href="https://fonts.googleapis.com";
                link rel="preconnect" href="https://fonts.gstatic.com" crossorigin;
                link rel="stylesheet" href=(FONTS_URL);
                link rel="stylesheet" href="/static/styles.css";
            }
            body {
                div.shell {
                    (sidebar(pathname))
                    main.main { (content) }
                }
            }
        }
    }
}

fn sidebar(pathname: &str) -> Markup {
    html! {
        aside.sidebar {
            a.brand href="/sok" { "Sif" span.accent { "f" } "ra" }
            @for group in NAV.iter() {
                div {
                    div.nav-heading { (group.heading) }
                    @for item in group.items.iter() {
                        @let active = pathname == item.href
                            || pathname.starts_with(&format!("{}/", item.href));
                        a.nav-link.active[active] href=(item.href) aria-current=(active) {
                            span { (item.label) }
                            @if let Some(badge) = item.badge {
                                span.pill.pill-warn { (badge) }
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
    html! { span.example-badge { "EJEMPLO" } }
}

fn risk_pill(level: RiskLevel) -> Markup {
    let label = match level {
        Severity::Good => "Låg risk",
        Severity::Warn => "Bevaka",
        Severity::Bad => "Förhöjd risk",
    };
    html! { span class={"pill tracking pill-" (sev_class(level))} { (label) } }
}

fn status_pill(text: &str) -> Markup {
    html! { span.pill.pill-good { (text) } }
}

fn alert_row(severity: Severity, body: Markup) -> Markup {
    html! {
        div.alert-row {
            span class={"alert-dot dot-" (sev_class(severity))} {}
            span { (body) }
        }
    }
}

fn benchmark_bar(label: &str, pair: BenchmarkPair, scale_max: f64) -> Markup {
    let unit = " %";
    let value_pct = (pair.value / scale_max * 100.0).clamp(0.0, 100.0);
    let median_pct = (pair.median / scale_max * 100.0).clamp(0.0, 100.0);
    html! {
        div.bench-row {
            span { (label) }
            div.bench-track title=(format!("Median: {}{}", pair.median, unit)) {
                div.bench-fill style=(format!("width:{}%", value_pct)) {}
                span.bench-median style=(format!("left:{}%", median_pct)) {}
            }
            span.mono { (pair.value) (unit) }
        }
    }
}

/// Gráfico de barras SVG puro para 5 años de omsättning.
fn revenue_chart(revenue: &FiveYearSeries) -> Markup {
    let (w, h) = (520.0_f64, 210.0_f64);
    let (pl, pb, pt, pr) = (44.0_f64, 26.0_f64, 16.0_f64, 10.0_f64);
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

    html! {
        svg.chart viewBox=(format!("0 0 {} {}", w, h)) width="100%" role="img" aria-label="Omsättning, fem år" {
            @for (value, y) in grid_lines.iter() {
                g {
                    line x1=(pl) x2=(w - pr) y1=(y) y2=(y) stroke="var(--line)" stroke-width="1" {}
                    text x=(pl - 6.0) y=(y + 3.0) text-anchor="end" { (value / 1000.0) " mkr" }
                }
            }
            @for (i, v) in revenue.iter().enumerate() {
                @let v = *v as f64;
                @let bar_h = (h - pb - pt) * v / top;
                @let x = pl + i as f64 * bw + bw * 0.2;
                @let bar_w = bw * 0.6;
                @let y = h - pb - bar_h;
                @let is_last = i == revenue.len() - 1;
                g {
                    rect x=(x) y=(y) width=(bar_w) height=(bar_h)
                        fill=(if is_last { "var(--bar)" } else { "var(--bar2)" }) {}
                    text x=(x + bar_w / 2.0) y=(h - 8.0) text-anchor="middle" { (FINANCIAL_YEARS[i]) }
                    @if is_last {
                        text x=(x + bar_w / 2.0) y=(y - 5.0) text-anchor="middle" fill="var(--ink)" {
                            (format!("{:.1}", v / 1000.0))
                        }
                    }
                }
            }
        }
    }
}

/// Gráfico de barras SVG de la caja proyectada, a partir del saldo inicial y las entradas/salidas semanales.
fn cash_flow_chart(weeks: &[CashWeek], start_balance: i64) -> Markup {
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

    let (w, h) = (560.0_f64, 200.0_f64);
    let (pl, pb, pt) = (44.0_f64, 22.0_f64, 14.0_f64);
    let lo = (min.min(0)) as f64;
    let hi = (max as f64 / 100.0).ceil() * 100.0;
    let bw = (w - pl - 10.0) / weeks.len() as f64;

    let y = |v: f64| h - pb - (h - pb - pt) * (v - lo) / (hi - lo);
    let grid_values = [lo, js_round((lo + hi) / 2.0 / 100.0) * 100.0, hi];
    let lowest_idx = points.iter().position(|&p| p == min).unwrap_or(0);

    html! {
        div {
            svg.chart viewBox=(format!("0 0 {} {}", w, h)) width="100%" role="img" aria-label="Prognos för kassa, 13 veckor" {
                @for v in grid_values.iter() {
                    g {
                        line x1=(pl) x2=(w - 10.0) y1=(y(*v)) y2=(y(*v)) stroke="var(--line)" {}
                        text x=(pl - 6.0) y=(y(*v) + 3.0) text-anchor="end" { (v) }
                    }
                }
                @for (i, v) in points.iter().enumerate() {
                    @let v = *v;
                    @let x = pl + i as f64 * bw + bw * 0.15;
                    @let bar_w = bw * 0.7;
                    @let y0 = y(0.0);
                    @let y1 = y(v as f64);
                    g {
                        rect x=(x) y=(y0.min(y1)) width=(bar_w) height=((y1 - y0).abs())
                            fill=(if v < 150 { "var(--bad)" } else { "var(--bar)" }) {}
                        text x=(x + bar_w / 2.0) y=(h - 6.0) text-anchor="middle" { "v" (weeks[i].week) }
                    }
                }
            }
            p.mt-2.text-sm {
                "Kassan kommer nära " strong { (min) " tkr" } " i vecka " (weeks[lowest_idx].week) "."
            }
        }
    }
}

// ───────────────────────── Páginas ─────────────────────────

pub fn sok_page(query: &str) -> Markup {
    let results = search_example_companies(query);
    layout(
        "Sök företag — Siffra",
        "/sok",
        html! {
            div {
                h2.page-title.mb-1 { "Sök företag" }
                p.lead {
                    "Interfaz de producto en sueco. Estas tres empresas son un " strong { "EJEMPLO" }
                    " ficticio; en producción esta lista vendrá de SCB y Bolagsverket."
                }
                form.search method="get" action="/sok" {
                    input.search-input type="search" name="q" value=(query)
                        placeholder="Sök på namn, organisationsnummer eller ort"
                        aria-label="Sök företag" autocomplete="off";
                }
                div.table-wrap {
                    table {
                        thead {
                            tr {
                                th { "Företag" }
                                th { "Ort" }
                                th.right { "Omsättning 2024 (tkr)" }
                                th.right { "Resultat (tkr)" }
                                th { "Risk" }
                            }
                        }
                        tbody {
                            @for c in results.iter() {
                                tr {
                                    td.top {
                                        a.company-link href=(format!("/foretag/{}", c.org_number)) { (c.name) }
                                        div.org-sub { (c.org_number) }
                                    }
                                    td.top { (c.city) }
                                    td.top.right.mono { (format_tkr(c.financials.revenue[4])) }
                                    td.top.right.mono { (format_tkr(c.financials.result[4])) }
                                    td.top { (risk_pill(c.risk_level)) }
                                }
                            }
                            @if results.is_empty() {
                                tr {
                                    td colspan="5" class="muted" { "Inga träffar. Prova ett annat namn eller organisationsnummer." }
                                }
                            }
                        }
                    }
                }
                p.note { (example_badge()) " Tres empresas ficticias. En producción esta lista viene de SCB y Bolagsverket." }
                // Filtrado "en vivo" como el original: reenvía el formulario al escribir (con retardo).
                script { (maud::PreEscaped(SEARCH_SCRIPT)) }
            }
        },
    )
}

const SEARCH_SCRIPT: &str = r#"(function(){var i=document.querySelector('.search-input');if(!i)return;
if(i.value){i.focus();var n=i.value.length;try{i.setSelectionRange(n,n);}catch(e){}}
var t;i.addEventListener('input',function(){clearTimeout(t);t=setTimeout(function(){i.form.submit();},300);});})();"#;

pub fn company_page(company: &Company, tab: &str) -> Markup {
    let f = &company.financials;
    let growth = (f.revenue[4] as f64 / f.revenue[3] as f64 - 1.0) * 100.0;
    let margin = f.result[4] as f64 / f.revenue[4] as f64 * 100.0;
    let solidity = f.equity[4] as f64 / f.total_assets[4] as f64 * 100.0;

    layout(
        DEFAULT_TITLE,
        &format!("/foretag/{}", company.org_number),
        html! {
            div {
                a.back href="/sok" { "← Sökresultat" }

                div.company-head {
                    div {
                        h3.company-name { (company.name) }
                        div.company-meta {
                            "Org.nr " (company.org_number) " · " (company.legal_form) " · " (company.city)
                        }
                    }
                    div.pills {
                        (status_pill(company.status))
                        (risk_pill(company.risk_level))
                    }
                }

                div.kpis {
                    (kpi("Omsättning", &format!("{} tkr", format_tkr(f.revenue[4])),
                        &format!("{} {:.1} % mot 2023", if growth >= 0.0 { "▲" } else { "▼" }, growth.abs()),
                        if growth >= 0.0 { "text-good" } else { "text-bad" }))
                    (kpi("Resultat", &format!("{} tkr", format_tkr(f.result[4])),
                        &format!("Vinstmarginal {:.1} %", margin), "text-ink2"))
                    (kpi("Soliditet", &format!("{:.1} %", solidity),
                        &format!("Eget kapital {} tkr", format_tkr(f.equity[4])), "text-ink2"))
                    (kpi("Anställda", company.employee_range, "SCB, intervall", "text-ink2"))
                }

                (company_tabs(company, tab))
            }
        },
    )
}

fn kpi(eyebrow: &str, value: &str, detail: &str, detail_class: &str) -> Markup {
    html! {
        div.kpi {
            div.kpi-eyebrow { (eyebrow) }
            div.kpi-value { (value) }
            div class={"kpi-detail " (detail_class)} { (detail) }
        }
    }
}

const SUB_TABS: [(&str, &str); 4] = [
    ("ov", "Översikt"),
    ("fin", "Bokslut"),
    ("ppl", "Personer"),
    ("ai", "Sammanfattning"),
];

fn company_tabs(company: &Company, tab: &str) -> Markup {
    // Pestaña desconocida → Översikt (la inicial del original).
    let active = SUB_TABS.iter().find(|(id, _)| *id == tab).map(|(id, _)| *id).unwrap_or("ov");
    html! {
        div {
            div.tablist role="tablist" {
                @for (id, label) in SUB_TABS.iter() {
                    a.tab role="tab" aria-selected=(*id == active)
                        href=(format!("/foretag/{}?tab={}", company.org_number, id)) { (label) }
                }
            }
            @match active {
                "fin" => { (financials(company)) }
                "ppl" => { (people(company)) }
                "ai" => { (summary(company)) }
                _ => { (overview(company)) }
            }
        }
    }
}

fn overview(company: &Company) -> Markup {
    let sni_code: String = company.sni.chars().take(6).collect();
    html! {
        div {
            div.overview-grid {
                div.card {
                    h4.eyebrow { "Omsättning, 5 år (tkr)" }
                    (revenue_chart(&company.financials.revenue))
                }
                div.card {
                    h4.eyebrow { "Mot branschen (SNI-median)" }
                    (benchmark_bar("Vinstmarginal", company.benchmarks.margin, 30.0))
                    (benchmark_bar("Soliditet", company.benchmarks.solidity, 80.0))
                    (benchmark_bar("Kassalikviditet", company.benchmarks.liquidity, 250.0))
                    p.note.mt-10 { "Strecket visar medianen för SNI " (sni_code) "." }
                }
            }
            div.card.mt-4 {
                h4.eyebrow { "Signaler" }
                @for a in company.alerts.iter() {
                    (alert_row(a.severity, html! { (a.text) }))
                }
            }
            p.note { "Verksamhet: " (company.sni) }
        }
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
        div {
            div.table-wrap {
                table {
                    thead {
                        tr {
                            th { "tkr" }
                            @for y in FINANCIAL_YEARS.iter() { th.right { (y) } }
                        }
                    }
                    tbody {
                        @for (label, values) in rows.iter() {
                            tr {
                                td { (label) }
                                @for v in values.iter() {
                                    td.right.mono.text-bad[*v < 0] { (format_tkr(*v)) }
                                }
                            }
                        }
                    }
                }
            }
            p.note { "Källa: digitalt inlämnade årsredovisningar (iXBRL), Bolagsverket. " (example_badge()) }
            button.btn-disabled.mt-2 disabled { "Ladda ner årsredovisning (zip)" }
        }
    }
}

fn people(company: &Company) -> Markup {
    html! {
        div {
            div.table-wrap {
                table {
                    thead { tr { th { "Roll" } th { "Namn" } } }
                    tbody {
                        @for p in company.people.iter() {
                            tr { td { (p.role) } td { (p.name) } }
                        }
                    }
                }
            }
            p.mt-10 { "Firmateckning: " strong { (company.firmateckning) } }
            p.note.mt-2 { "Kräver Bolagsverkets betalda API (fas 2). Namn är påhittade. " (example_badge()) }
        }
    }
}

fn summary(company: &Company) -> Markup {
    html! {
        div.card {
            h4.eyebrow { "Automatisk sammanfattning" }
            p.summary-text { (generate_example_summary(company)) }
            p.note.mt-2 { "Genererad från siffrorna på fliken Bokslut. Ingen kreditbedömning. " (example_badge()) }
        }
    }
}

pub fn company_not_found_page() -> Markup {
    layout(
        DEFAULT_TITLE,
        "/foretag",
        html! {
            div {
                h2.page-title.mb-2 { "Företaget hittades inte" }
                p.lead.mb-3 {
                    "I det här scaffoldet finns bara de tre EJEMPLO-företagen. Sök på namn, organisationsnummer eller ort för att hitta dem."
                }
                a.accent-link href="/sok" { "← Tillbaka till sökningen" }
            }
        },
    )
}

pub fn not_found_page() -> Markup {
    layout(
        "404 — Siffra",
        "",
        html! {
            div {
                h2.page-title.mb-2 { "404 — Sidan hittades inte" }
                a.accent-link href="/sok" { "← Tillbaka till sökningen" }
            }
        },
    )
}

pub fn bevakning_page() -> Markup {
    layout(
        "Bevakning — Siffra",
        "/bevakning",
        html! {
            div {
                h2.page-title.mb-3 { "Bevakning" }
                div.card.narrow {
                    @for a in EXAMPLE_WATCH_ALERTS.iter() {
                        (alert_row(a.severity, html! {
                            strong { (a.company_name) }
                            br;
                            (a.text) " " span.muted { "· " (a.when) }
                        }))
                    }
                }
                p.note {
                    (example_badge())
                    " Mail varje måndag med det som ändrats. Fase 2 del plan de trabajo — depende de notificaciones de Bolagsverket (API de pago) o comparación semanal del archivo gratuito."
                }
            }
        },
    )
}

pub fn likviditet_page() -> Markup {
    layout(
        "Likviditetsprognos — Siffra",
        "/likviditet",
        html! {
            div {
                h2.page-title.mb-3 { "Likviditetsprognos, 13 veckor" }
                div.card.narrow {
                    h4.eyebrow { "Förväntad kassa (tkr)" }
                    (cash_flow_chart(&EXAMPLE_CASH_WEEKS, EXAMPLE_CASH_START_BALANCE))
                }
                div.card.narrow.mt-3 {
                    (alert_row(Severity::Warn, html! {
                        "Momsinbetalning och löner infaller samma vecka som el mínimo de caja."
                    }))
                }
                p.note {
                    (example_badge())
                    " Beräknat från kund- och leverantörsfakturor och återkommande betalningar. Fase 3 del plan de trabajo — requiere importación SIE."
                }
            }
        },
    )
}

pub fn sie_page() -> Markup {
    layout(
        "Importera SIE — Siffra",
        "/sie",
        html! {
            div {
                h2.page-title.mb-3 { "Importera bokföring (SIE)" }
                div.dropzone {
                    strong { "Släpp en SIE-fil här" }
                    p { "SIE 4 från Fortnox, Visma, Bokio eller annat program." }
                    button.btn-disabled disabled { "Välj fil" }
                }
                div.card.narrow.mt-3 {
                    h4.eyebrow { "Förhandsvisning" }
                    div.scroll-x {
                        table {
                            thead {
                                tr {
                                    th { "Konto (BAS)" }
                                    th { "Namn" }
                                    th.right { "Saldo" }
                                }
                            }
                            tbody {
                                @for row in EXAMPLE_SIE_PREVIEW.iter() {
                                    tr {
                                        td.mono { (row.account) }
                                        td { (row.name) }
                                        td.right.mono { (format_int(row.balance_sek)) }
                                    }
                                }
                            }
                        }
                    }
                }
                p.note {
                    (example_badge())
                    " No hay parser SIE real todavía (Fase 3). Este es solo el layout de la pantalla."
                }
            }
        },
    )
}

pub fn fakturor_page() -> Markup {
    layout(
        "Kundfakturor — Siffra",
        "/fakturor",
        html! {
            div {
                h2.page-title.mb-3 { "Kundfakturor" }
                div.table-wrap {
                    table {
                        thead {
                            tr {
                                th { "Nr" }
                                th { "Kund" }
                                th.right { "Belopp (kr)" }
                                th { "Förfaller" }
                                th { "Status" }
                            }
                        }
                        tbody {
                            @for inv in EXAMPLE_INVOICES.iter() {
                                tr {
                                    td.mono { (inv.number) }
                                    td { (inv.customer) }
                                    td.right.mono { (format_int(inv.amount_sek)) }
                                    td { (inv.due_date) }
                                    td { span class={"pill pill-" (sev_class(inv.severity))} { (inv.status) } }
                                }
                            }
                        }
                    }
                }
                p.note {
                    (example_badge())
                    " Fase 3+ del plan de trabajo. El cliente podrá ser avisado si el pagador tiene riesgo elevado."
                }
            }
        },
    )
}
