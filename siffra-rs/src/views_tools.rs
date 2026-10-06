//! Pantallas de trabajo con empresas reales: Mis empresas (seguimiento), Comparar e Historial.

use maud::{html, Markup};

use crate::analysis::{parse_level, Snapshot, FLAG_DEREGISTERED, FLAG_INSOLVENCY};
use crate::bolagsverket::Organisation;
use crate::db::{WatchRow, WATCH_LIMIT};
use crate::scb::SectorMedians;
use crate::util::{days_since, urlencode};
use crate::views::{format_orgnr, icon, icon_sized, layout, risk_pill_opt, Ctx};
use crate::views_admin::time_tag;

/// Mensaje de resultado de una acción (arriba de la página).
pub struct Flash<'a> {
    pub ok: bool,
    pub text: &'a str,
}

fn flash_box(f: Option<&Flash>) -> Markup {
    html! {
        @if let Some(f) = f {
            @if f.ok {
                p.form-ok role="status" { (icon_sized("check-circle", "icon-sm")) (f.text) }
            } @else {
                p.form-error role="alert" { (icon_sized("octagon-alert", "icon-sm")) (f.text) }
            }
        }
    }
}

/// Botón de la ficha para seguir o dejar de seguir una empresa.
pub fn follow_button(c: &Ctx, orgnr: &str, following: bool) -> Markup {
    let action = if following { "/bevakning/remove" } else { "/bevakning/add" };
    html! {
        form.follow-form method="post" action=(action) {
            (c.csrf_input())
            input type="hidden" name="orgnr" value=(orgnr);
            input type="hidden" name="next" value=(format!("/foretag/{orgnr}"));
            button.btn.follow-btn.following[following] type="submit" aria-pressed=(if following { "true" } else { "false" }) data-tip=(c.t("watch.follow_tip")) {
                (icon_sized("star", "icon-sm"))
                (c.t(if following { "watch.unfollow" } else { "watch.follow" }))
            }
        }
    }
}

fn level_key(level: Option<&str>) -> &'static str {
    match level {
        Some("good") => "risk.low",
        Some("warn") => "risk.watch",
        Some("bad") => "risk.high",
        Some(_) => "risk.unknown",
        None => "my.pending",
    }
}

fn flags_pill(c: &Ctx, flags: i64) -> Markup {
    if flags & FLAG_INSOLVENCY != 0 {
        html! { span.pill.pill-bad { span.pill-dot aria-hidden="true" {} (c.t("my.status.insolvency")) } }
    } else if flags & FLAG_DEREGISTERED != 0 {
        html! { span.pill.pill-bad { span.pill-dot aria-hidden="true" {} (c.t("my.status.deregistered")) } }
    } else {
        html! { span.pill.pill-good { span.pill-dot aria-hidden="true" {} (c.t("status.active")) } }
    }
}

fn dash() -> Markup {
    html! { span.muted { "—" } }
}

fn change_cell(c: &Ctx, r: &WatchRow) -> Markup {
    let Some(kind) = r.change_kind.as_deref() else { return dash() };
    let when = r.changed_at.as_deref().unwrap_or("");
    let recent = days_since(when) <= 30;
    let text = match kind {
        "level" => c.tf(
            "my.change.level",
            &[c.t(level_key(r.change_from.as_deref())), c.t(level_key(r.change_to.as_deref()))],
        ),
        _ => c.t("my.change.status").to_string(),
    };
    let worse = kind == "level" && r.change_to.as_deref() == Some("bad");
    html! {
        div.change.new[recent].worse[recent && worse] {
            (text)
            div.org-sub { (time_tag(when)) }
        }
    }
}

fn watch_row(c: &Ctx, r: &WatchRow) -> Markup {
    let level = parse_level(r.level.as_deref().unwrap_or(""));
    html! {
        tr {
            td.top.check-cell {
                input type="checkbox" name="o" value=(r.orgnr) form="cmp-form" aria-label=(c.tf("my.select_aria", &[&r.name]));
            }
            td.top {
                a.company-link href=(format!("/foretag/{}", r.orgnr)) { (r.name) }
                div.org-sub { (format_orgnr(&r.orgnr)) @if !r.form.is_empty() { " · " (r.form) } }
            }
            td.top { (flags_pill(c, r.flags)) }
            td.top {
                @if r.level.is_none() {
                    span.pill.pill-user { (c.t("my.pending")) }
                } @else {
                    (risk_pill_opt(c, level))
                }
            }
            td.top.right.num.mono.hide-sm {
                @if let Some(v) = r.revenue { (c.int(v)) @if let Some(y) = &r.year { div.org-sub { (y) } } } @else { "—" }
            }
            td.top.right.num.mono.hide-sm.neg[r.result.is_some_and(|v| v < 0)] { @if let Some(v) = r.result { (c.int(v)) } @else { "—" } }
            td.top.right.num.mono.hide-sm { @if let Some(s) = r.solidity { (c.pct1(s)) } @else { "—" } }
            td.top { (change_cell(c, r)) }
            td.top.right {
                div.row-actions {
                    form method="post" action="/bevakning/refresh" {
                        (c.csrf_input())
                        input type="hidden" name="orgnr" value=(r.orgnr);
                        button.icon-link type="submit" data-tip=(c.t("my.refresh")) aria-label=(c.tf("my.refresh_aria", &[&r.name])) { (icon("refresh")) }
                    }
                    form method="post" action="/bevakning/remove" {
                        (c.csrf_input())
                        input type="hidden" name="orgnr" value=(r.orgnr);
                        input type="hidden" name="next" value="/bevakning";
                        button.icon-link.danger type="submit" data-tip=(c.t("watch.unfollow")) aria-label=(c.tf("my.remove_aria", &[&r.name])) { (icon("trash")) }
                    }
                }
            }
        }
    }
}

pub fn watch_page(c: &Ctx, rows: &[WatchRow], flash: Option<&Flash>) -> Markup {
    let unit = c.t("unit.tkr");
    layout(
        c,
        c.t("nav.watch"),
        html! {
            div.page-head.page-head-row {
                div {
                    h1.page-title { (c.t("my.title")) }
                    p.lead { (c.t("my.lead")) }
                }
                @if !rows.is_empty() {
                    form method="post" action="/bevakning/refresh" {
                        (c.csrf_input())
                        input type="hidden" name="orgnr" value="all";
                        button.btn type="submit" { (icon_sized("refresh", "icon-sm")) (c.t("my.refresh_all")) }
                    }
                }
            }
            (flash_box(flash))
            @if rows.is_empty() {
                div.card.glass.narrow.intro {
                    h2.card-title { (c.t("my.empty.title")) }
                    p { (c.t("my.empty.text")) }
                    div.form-actions { a.btn.btn-primary href="/sok" { (icon_sized("search", "icon-sm")) (c.t("nf.to_search")) } }
                }
            } @else {
                form #cmp-form method="get" action="/comparar" {}
                p.result-count role="status" {
                    (c.tf(if rows.len() == 1 { "my.count.one" } else { "my.count.many" }, &[&rows.len().to_string(), &WATCH_LIMIT.to_string()]))
                }
                div.table-wrap {
                    table.row-link.watch-table {
                        caption.sr-only { (c.t("my.title")) }
                        thead { tr {
                            th.check-cell scope="col" { span.sr-only { (c.t("my.select")) } }
                            th scope="col" { (c.t("sok.col.company")) }
                            th scope="col" { (c.t("usr.col.status")) }
                            th scope="col" { (c.t("sok.col.risk")) }
                            th.right.hide-sm scope="col" { (c.tf("sok.col.revenue", &[unit])) }
                            th.right.hide-sm scope="col" { (c.tf("sok.col.result", &[unit])) }
                            th.right.hide-sm scope="col" { (c.t("term.solidity")) }
                            th scope="col" { (c.t("my.col.change")) }
                            th.right scope="col" { (c.t("usr.col.actions")) }
                        } }
                        tbody { @for r in rows { (watch_row(c, r)) } }
                    }
                }
                div.form-actions {
                    button.btn type="submit" form="cmp-form" { (icon_sized("columns", "icon-sm")) (c.t("my.compare_selected")) }
                }
                p.note { (c.t("my.note")) }
            }
        },
    )
}

// ───────────────────────── Comparar ─────────────────────────

pub struct CompareData {
    pub org: Organisation,
    pub snap: Snapshot,
    pub medians: Option<SectorMedians>,
}

pub struct CompareCol {
    pub orgnr: String,
    /// `None` = no se pudo cargar (no existe, o el registro no responde).
    pub data: Option<CompareData>,
}

/// Marca el mayor valor de la fila cuando hay al menos dos con dato.
pub fn best_flags(values: &[Option<f64>]) -> Vec<bool> {
    let known: Vec<f64> = values.iter().flatten().copied().collect();
    if known.len() < 2 {
        return vec![false; values.len()];
    }
    let max = known.iter().cloned().fold(f64::MIN, f64::max);
    values.iter().map(|v| v.is_some_and(|x| (x - max).abs() < 1e-9)).collect()
}

pub fn compare_page(c: &Ctx, inputs: &[String], cols: &[CompareCol], notice: Option<&str>) -> Markup {
    let ready: Vec<&CompareData> = cols.iter().filter_map(|col| col.data.as_ref()).collect();
    let metric_row = |label: String, values: Vec<Option<f64>>, fmt: &dyn Fn(f64) -> String| -> Markup {
        let best = best_flags(&values);
        html! {
            tr {
                th scope="row" { (label) }
                @for (v, b) in values.iter().zip(best) {
                    td.right.num.mono.best[b].neg[v.is_some_and(|x| x < 0.0)] { @if let Some(x) = v { (fmt(*x)) } @else { span.muted { "—" } } }
                }
            }
        }
    };
    let money = |x: f64| c.int(x.round() as i64);
    let pct = |x: f64| c.pct1(x);
    layout(
        c,
        c.t("nav.compare"),
        html! {
            div.page-head {
                h1.page-title { (c.t("cmp.title")) }
                p.lead { (c.t("cmp.lead")) }
            }
            form.compare-form method="get" action="/comparar" {
                @for (i, v) in inputs.iter().enumerate() {
                    div.compare-field {
                        label.field-label for=(format!("o{i}")) { (c.tf("cmp.label", &[&(i + 1).to_string()])) }
                        input.text-input id=(format!("o{i}")) type="text" name="o" value=(v) placeholder="556703-7485" autocomplete="off" inputmode="numeric" spellcheck="false";
                    }
                }
                button.btn.btn-primary type="submit" { (icon_sized("columns", "icon-sm")) (c.t("cmp.submit")) }
            }
            @if let Some(n) = notice { p.form-error role="alert" { (icon_sized("octagon-alert", "icon-sm")) (n) } }
            @if !cols.is_empty() && !ready.is_empty() {
                div.table-wrap.mt-4 {
                    table.cmp-table {
                        caption.sr-only { (c.t("cmp.title")) }
                        thead { tr {
                            th scope="col" { span.sr-only { (c.t("cmp.metric")) } }
                            @for col in cols {
                                th.right scope="col" {
                                    @match &col.data {
                                        Some(d) => {
                                            a.company-head-link href=(format!("/foretag/{}", d.org.organisationsnummer)) { (d.org.namn) }
                                            div.org-sub { (format_orgnr(&d.org.organisationsnummer)) }
                                        }
                                        None => { span.neg { (c.tf("cmp.not_found", &[&col.orgnr])) } }
                                    }
                                }
                            }
                        } }
                        tbody {
                            tr {
                                th scope="row" { (c.t("cmp.row.form")) }
                                @for col in cols { td.right { @match &col.data { Some(d) => { (d.org.organisationsform) } None => { (dash()) } } } }
                            }
                            tr {
                                th scope="row" { (c.t("usr.col.status")) }
                                @for col in cols { td.right { @match &col.data { Some(d) => { (flags_pill(c, d.snap.flags)) } None => { (dash()) } } } }
                            }
                            tr {
                                th scope="row" { (c.t("sok.col.risk")) }
                                @for col in cols { td.right { @match &col.data { Some(d) => { (risk_pill_opt(c, d.snap.level)) } None => { (dash()) } } } }
                            }
                            tr {
                                th scope="row" { (c.t("cmp.row.year")) }
                                @for col in cols { td.right.num.mono { @match col.data.as_ref().and_then(|d| d.snap.year.as_deref()) { Some(y) => { (y) } None => { (dash()) } } } }
                            }
                            (metric_row(c.tf("sok.col.revenue", &[c.t("unit.tkr")]), cols.iter().map(|k| k.data.as_ref().and_then(|d| d.snap.revenue).map(|v| v as f64)).collect(), &money))
                            (metric_row(c.tf("sok.col.result", &[c.t("unit.tkr")]), cols.iter().map(|k| k.data.as_ref().and_then(|d| d.snap.result).map(|v| v as f64)).collect(), &money))
                            (metric_row(c.t("fin.equity").to_string(), cols.iter().map(|k| k.data.as_ref().and_then(|d| d.snap.equity).map(|v| v as f64)).collect(), &money))
                            (metric_row(c.t("term.solidity").to_string(), cols.iter().map(|k| k.data.as_ref().and_then(|d| d.snap.solidity)).collect(), &pct))
                            (metric_row(c.t("term.margin").to_string(), cols.iter().map(|k| k.data.as_ref().and_then(|d| d.snap.margin)).collect(), &pct))
                            (metric_row(c.t("cmp.row.growth").to_string(), cols.iter().map(|k| k.data.as_ref().and_then(|d| d.snap.growth)).collect(), &pct))
                            tr.sector {
                                th scope="row" { (c.t("cmp.row.sector")) }
                                @for col in cols {
                                    td.right.num.mono {
                                        @match col.data.as_ref().and_then(|d| d.medians.as_ref()) {
                                            Some(m) => { (c.t("term.solidity")) " " (c.pct1(m.solidity)) br; (c.t("term.margin")) " " (c.pct1(m.margin)) div.org-sub { "SNI " (m.sni_code) } }
                                            None => { (dash()) }
                                        }
                                    }
                                }
                            }
                        }
                    }
                }
                p.note { (c.t("cmp.note")) }
            }
        },
    )
}

// ───────────────────────── Historial ─────────────────────────

pub struct SeenCompany {
    pub orgnr: String,
    /// Nombre si se conoce (índice del registro o empresas seguidas).
    pub name: Option<String>,
    pub when: String,
}

pub fn history_page(c: &Ctx, companies: &[SeenCompany], searches: &[(String, String)]) -> Markup {
    layout(
        c,
        c.t("nav.history"),
        html! {
            div.page-head {
                h1.page-title { (c.t("his.title")) }
                p.lead { (c.t("his.lead")) }
            }
            div.grid-2 {
                div.card.glass {
                    h2.card-title { (c.t("his.companies")) }
                    @if companies.is_empty() {
                        p.muted { (c.t("his.empty")) }
                    } @else {
                        ul.history-list {
                            @for s in companies {
                                li {
                                    a.history-link href=(format!("/foretag/{}", s.orgnr)) {
                                        strong { (s.name.clone().unwrap_or_else(|| format_orgnr(&s.orgnr))) }
                                        @if s.name.is_some() { span.org-sub { (format_orgnr(&s.orgnr)) } }
                                    }
                                    span.muted.history-when { (time_tag(&s.when)) }
                                }
                            }
                        }
                    }
                }
                div.card.glass {
                    h2.card-title { (c.t("his.searches")) }
                    @if searches.is_empty() {
                        p.muted { (c.t("his.empty")) }
                    } @else {
                        ul.history-list {
                            @for (q, when) in searches {
                                li {
                                    a.history-link href=(format!("/sok?q={}", urlencode(q))) {
                                        strong { (q) }
                                    }
                                    span.muted.history-when { (time_tag(when)) }
                                }
                            }
                        }
                    }
                }
            }
        },
    )
}

