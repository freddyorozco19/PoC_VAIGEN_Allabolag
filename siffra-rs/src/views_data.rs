//! Pantalla "Datos": la estructura de todas las fuentes que alimentan Siffra, lo que hay guardado y un
//! explorador de todos los conceptos del dataset. La ven solo los administradores.
//!
//! Las descripciones de las fuentes son estáticas (reflejan lo que lee el código); las cifras y el explorador
//! salen de lo realmente guardado (`report` + `fact`) y del índice de nombres.

use maud::{html, Markup};

use crate::annual_report::mapped_members;
use crate::db::{ConceptRow, DataStats};
use crate::registry::Stats as RegistryStats;
use crate::util::urlencode;
use crate::views::{icon_sized, info_btn, layout, Ctx};

/// Un campo de una fuente: nombre tal como lo entrega, tipo, significado (clave del catálogo) y para qué se usa.
struct Field {
    name: &'static str,
    typ: &'static str,
    meaning: &'static str,
    uses: &'static [&'static str],
}

const fn f(name: &'static str, typ: &'static str, meaning: &'static str, uses: &'static [&'static str]) -> Field {
    Field { name, typ, meaning, uses }
}

const BV_ORG: &[Field] = &[
    f("organisationsidentitet.identitetsbeteckning", "text", "data.bv.id", &["profile", "search"]),
    f("organisationsnamn.organisationsnamnLista[]", "list", "data.bv.names", &["profile"]),
    f("organisationsform.kod / .klartext", "code + text", "data.bv.form", &["profile", "compare"]),
    f("juridiskForm.kod / .klartext", "code + text", "data.bv.legal", &["none"]),
    f("verksamOrganisation.kod", "JA / NEJ", "data.bv.active", &["profile", "assessment"]),
    f("postadressOrganisation.postadress", "object", "data.bv.address", &["profile"]),
    f("organisationsdatum.registreringsdatum", "date", "data.bv.registered", &["profile"]),
    f("avregistreradOrganisation.avregistreringsdatum", "date", "data.bv.deregistered", &["profile", "assessment"]),
    f("avregistreringsorsak", "code + text", "data.bv.reason", &["none"]),
    f("pagaendeAvvecklingsEllerOmstruktureringsforfarande[]", "list", "data.bv.proceedings", &["profile", "assessment"]),
    f("verksamhetsbeskrivning.beskrivning", "text", "data.bv.activity", &["profile"]),
    f("naringsgrenOrganisation.sni[]", "list of code + text", "data.bv.sni", &["profile", "assessment"]),
    f("registreringsland, reklamsparr, namnskyddslopnummer", "misc", "data.bv.other", &["none"]),
    f("‹block›.fel / ‹block›.dataproducent", "text", "data.bv.meta", &["none"]),
];

const BV_DOCS: &[Field] = &[
    f("dokument[].dokumentId", "text", "data.bd.id", &["stored"]),
    f("dokument[].filformat", "text", "data.bd.format", &["none"]),
    f("dokument[].rapporteringsperiodTom", "date", "data.bd.period", &["charts", "stored"]),
    f("dokument[].registreringstidpunkt", "date", "data.bd.registered", &["stored"]),
    f("ix:nonFraction  (name, contextRef, unitRef, scale, sign, format)", "number", "data.bd.number", &["charts", "assessment", "stored"]),
    f("ix:nonNumeric  (name, contextRef)", "text", "data.bd.text", &["people", "stored"]),
    f("xbrli:context  (period, instant, dimensions)", "context", "data.bd.context", &["stored"]),
    f("xbrli:unit", "text", "data.bd.unit", &["stored"]),
    f("Underskrift*  (signatories)", "text", "data.bd.signatories", &["people"]),
];

const REGISTRY_COLS: &[Field] = &[
    f("orgnr", "text", "data.rg.orgnr", &["search"]),
    f("id_type", "text", "data.rg.id_type", &["search"]),
    f("seq", "integer", "data.rg.seq", &["none"]),
    f("name", "text", "data.rg.name", &["search"]),
    f("names", "text", "data.rg.names", &["search"]),
    f("form", "text", "data.rg.form", &["search"]),
    f("dereg", "date", "data.rg.dereg", &["search"]),
    f("dereg_reason", "text", "data.rg.dereg_reason", &["search"]),
    f("registered", "date", "data.rg.registered", &["search"]),
    f("city", "text", "data.rg.city", &["search"]),
    f("postcode", "text", "data.rg.postcode", &["none"]),
];

const SCB: &[Field] = &[
    f("margin", "% (median)", "data.sc.margin", &["assessment"]),
    f("solidity", "% (median)", "data.sc.solidity", &["assessment"]),
    f("liquidity", "% (median)", "data.sc.liquidity", &["assessment"]),
    f("year", "text", "data.sc.year", &["assessment"]),
    f("sni_code / sni_label", "text", "data.sc.sni", &["assessment"]),
    f("size_class", "text", "data.sc.size", &["assessment"]),
    f("exact_sni / exact_size", "boolean", "data.sc.exact", &["assessment"]),
];

const GLEIF: &[Field] = &[
    f("data[].id", "LEI (20)", "data.gl.lei", &["stored"]),
    f("attributes.entity.legalName.name", "text", "data.gl.name", &["none"]),
    f("attributes.entity.registeredAs", "text", "data.gl.registered_as", &["stored"]),
    f("attributes.entity.registeredAt.id", "text", "data.gl.registered_at", &["stored"]),
    f("attributes.registration.status", "text", "data.gl.status", &["stored"]),
];

const XBRL: &[Field] = &[
    f("data[].attributes.period_end", "date", "data.xb.period", &["charts", "stored"]),
    f("data[].attributes.date_added", "date", "data.xb.added", &["stored"]),
    f("data[].attributes.json_url", "path", "data.xb.json_url", &["stored"]),
    f("data[].attributes.country", "text", "data.xb.country", &["stored"]),
    f("data[].attributes.error_count / warning_count / inconsistency_count", "integer", "data.xb.counts", &["none"]),
    f("facts{id}.value", "text / number", "data.xb.value", &["charts", "assessment", "stored"]),
    f("facts{id}.dimensions.concept", "text", "data.xb.concept", &["charts", "stored"]),
    f("facts{id}.dimensions.period", "text", "data.xb.period_fact", &["stored"]),
    f("facts{id}.dimensions.unit", "text", "data.xb.unit", &["stored"]),
    f("facts{id}.dimensions.‹axis›", "text", "data.xb.axis", &["stored"]),
];

const T_REPORT: &[Field] = &[
    f("doc_id", "text (key)", "data.tr.doc_id", &["stored"]),
    f("orgnr", "text", "data.tr.orgnr", &["stored"]),
    f("period_end", "date", "data.tr.period_end", &["stored"]),
    f("registered", "date", "data.tr.registered", &["stored"]),
    f("fetched_at", "datetime", "data.tr.fetched_at", &["stored"]),
    f("fact_count", "integer", "data.tr.fact_count", &["stored"]),
    f("raw_path", "path", "data.tr.raw_path", &["stored"]),
];

const T_FACT: &[Field] = &[
    f("doc_id", "text", "data.tf.doc_id", &["stored"]),
    f("ctx", "text", "data.tf.ctx", &["people", "stored"]),
    f("concept", "text", "data.tf.concept", &["charts", "assessment", "stored"]),
    f("value", "number", "data.tf.value", &["charts", "assessment", "stored"]),
    f("text", "text", "data.tf.text", &["people", "stored"]),
    f("unit", "text", "data.tf.unit", &["stored"]),
    f("scale", "integer", "data.tf.scale", &["stored"]),
    f("instant / start / end", "date", "data.tf.period", &["charts", "stored"]),
    f("dims", "text", "data.tf.dims", &["stored"]),
];

const SAMPLE_BV_ORG: &str = r#"{ "organisationer": [ {
  "organisationsidentitet": { "identitetsbeteckning": "5560000001", "typ": { "kod": "ORGANISATIONSNUMMER" } },
  "organisationsnamn": { "organisationsnamnLista": [ { "namn": "Exempel AB", "organisationsnamntyp": { "kod": "FORETAGSNAMN" }, "registreringsdatum": "2001-05-10" } ] },
  "organisationsform": { "kod": "AB", "klartext": "Aktiebolag", "dataproducent": "Bolagsverket" },
  "verksamOrganisation": { "kod": "JA", "dataproducent": "SCB" },
  "avregistreradOrganisation": null,
  "pagaendeAvvecklingsEllerOmstruktureringsforfarande": null,
  "postadressOrganisation": { "postadress": { "utdelningsadress": "Exempelgatan 1", "postnummer": "11122", "postort": "STOCKHOLM" } },
  "organisationsdatum": { "registreringsdatum": "2001-05-10" },
  "verksamhetsbeskrivning": { "beskrivning": "Bolaget har till föremål för sin verksamhet att ..." },
  "naringsgrenOrganisation": { "sni": [ { "kod": "62010", "klartext": "Datorprogrammering" } ] }
} ] }"#;

const SAMPLE_BV_DOCS: &str = r#"POST /dokumentlista   { "identitetsbeteckning": "5560000001" }
{ "dokument": [ { "dokumentId": "0b3c…-…_paket", "filformat": "application/zip",
                  "rapporteringsperiodTom": "2025-12-31", "registreringstidpunkt": "2026-03-01" } ] }

GET /dokument/{dokumentId}   →   ZIP con un .xhtml (iXBRL):
<ix:nonFraction name="se-gen-base:Nettoomsattning" contextRef="period0" unitRef="SEK" scale="3" decimals="-3" format="ixt:numspacecomma">1 250</ix:nonFraction>
<xbrli:context id="period0"><xbrli:period><xbrli:startDate>2025-01-01</xbrli:startDate><xbrli:endDate>2025-12-31</xbrli:endDate></xbrli:period></xbrli:context>"#;

const SAMPLE_REGISTRY: &str = r#"5560000001$ORGNR-IDORG;;SE-LAND;Exempel AB$FORETAGSNAMN-ORGEN$2001-05-10;AB-ORGFO;;;;2001-05-10;Bolaget har till föremål …;Exempelgatan 1$$STOCKHOLM$11122$SE-LAND"#;

const SAMPLE_GLEIF: &str = r#"GET /api/v1/lei-records?filter[entity.registeredAs]=556000-0001
{ "data": [ { "id": "549300XXXXXXXXXXXX00",
    "attributes": { "entity": { "legalName": { "name": "Exempel AB" }, "registeredAs": "556000-0001", "registeredAt": { "id": "RA000544" } },
                    "registration": { "status": "ISSUED" } } } ] }"#;

const SAMPLE_XBRL: &str = r#"GET /api/entities/549300XXXXXXXXXXXX00/filings
{ "data": [ { "attributes": { "period_end": "2024-12-31", "date_added": "2025-05-08 11:26:57", "country": "SE",
                              "json_url": "/549300…/2024-12-31/ESEF/SE/0/exempel-2024-12-31-sv.json" } } ] }

GET {json_url}   (xBRL-JSON)
{ "facts": { "f1": { "value": "247880000000.0", "decimals": -6,
                     "dimensions": { "concept": "ifrs-full:RevenueFromContractsWithCustomers", "entity": "scheme:549300…",
                                     "period": "2024-01-01T00:00:00/2025-01-01T00:00:00", "unit": "iso4217:SEK" } } } }"#;

fn use_pill(c: &Ctx, key: &str) -> Markup {
    let (class, label) = match key {
        "profile" => ("pill-user", "data.use.profile"),
        "search" => ("pill-user", "data.use.search"),
        "assessment" => ("pill-admin", "data.use.assessment"),
        "charts" => ("pill-admin", "data.use.charts"),
        "compare" => ("pill-user", "data.use.compare"),
        "people" => ("pill-user", "data.use.people"),
        "stored" => ("pill-warn", "data.use.stored"),
        _ => ("pill-user", "data.use.none"),
    };
    html! { span class={"pill " (class)} { (c.t(label)) } " " }
}

fn field_table(c: &Ctx, caption: &str, fields: &[Field]) -> Markup {
    html! {
        div.table-wrap.flat {
            table.data-fields {
                caption.sr-only { (caption) }
                thead { tr {
                    th scope="col" { (c.t("data.col.field")) }
                    th.hide-sm scope="col" { (c.t("data.col.type")) }
                    th scope="col" { (c.t("data.col.meaning")) }
                    th scope="col" { (c.t("data.col.use")) }
                } }
                tbody {
                    @for fd in fields {
                        tr {
                            td.mono.nowrap { (fd.name) }
                            td.hide-sm.muted { (fd.typ) }
                            td { (c.t(fd.meaning)) }
                            td { @for u in fd.uses { (use_pill(c, u)) } }
                        }
                    }
                }
            }
        }
    }
}

/// Una fuente: título, datos de acceso, tabla de campos y un ejemplo de la respuesta.
fn source_card(c: &Ctx, id: &str, title: &str, about: &str, meta: &[(&str, String)], fields: &[Field], sample: &str, open: bool) -> Markup {
    html! {
        details.card.glass.data-source id=(id) open[open] {
            summary {
                span.data-source-title { (title) }
                span.muted.data-source-count { (c.tf("data.fields_count", &[&fields.len().to_string()])) }
            }
            p.lead { (about) }
            dl.data-meta {
                @for (k, v) in meta { div { dt { (c.t(k)) } dd.mono { (v) } } }
            }
            (field_table(c, title, fields))
            details.chart-data {
                summary { (c.t("data.sample")) }
                pre.data-sample { (sample) }
            }
        }
    }
}

pub struct DataView<'a> {
    pub stats: &'a DataStats,
    pub registry: Option<&'a RegistryStats>,
    pub concepts: &'a [ConceptRow],
    pub total_concepts: i64,
    pub q: &'a str,
    pub taxonomy: &'a str,
    pub page: i64,
    pub per_page: i64,
}

fn explorer_query(v: &DataView, page: i64) -> String {
    let mut parts: Vec<String> = Vec::new();
    if !v.q.is_empty() {
        parts.push(format!("q={}", urlencode(v.q)));
    }
    if !v.taxonomy.is_empty() {
        parts.push(format!("tax={}", urlencode(v.taxonomy)));
    }
    if page > 1 {
        parts.push(format!("page={page}"));
    }
    parts.join("&")
}

fn example_text(c: &Ctx, r: &ConceptRow) -> String {
    match (r.example_value, &r.example_text) {
        (Some(v), _) => {
            let n = if v.fract() == 0.0 && v.abs() < 1e15 { c.int(v as i64) } else { c.dec(v, 4) };
            match r.example_unit.as_deref() {
                Some(u) => format!("{n} {u}"),
                None => n,
            }
        }
        (None, Some(t)) => format!("«{t}»"),
        _ => "—".to_string(),
    }
}

fn kpi(label: &str, value: String, hint: Option<String>) -> Markup {
    html! { div.stat { dt { (label) } dd { (value) } @if let Some(h) = hint { p.muted.stat-hint { (h) } } } }
}

pub fn data_page(c: &Ctx, v: &DataView) -> Markup {
    let s = v.stats;
    let pages = ((v.total_concepts + v.per_page - 1) / v.per_page).max(1);
    let page_url = |p: i64| {
        let q = explorer_query(v, p);
        if q.is_empty() { "/datos#explorer".to_string() } else { format!("/datos?{q}#explorer") }
    };
    let csv_url = {
        let q = explorer_query(v, 1);
        if q.is_empty() { "/datos.csv".to_string() } else { format!("/datos.csv?{q}") }
    };
    let bolagsverket_reports = s.reports - s.esef_reports;
    layout(
        c,
        c.t("nav.data"),
        html! {
            div.page-head.page-head-row {
                div {
                    h1.page-title { (c.t("data.title")) }
                    p.lead { (c.t("data.lead")) }
                }
                a.btn href=(csv_url) { (icon_sized("download", "icon-sm")) (c.t("data.export")) }
            }

            // Lo que hay guardado
            dl.stats {
                (kpi(c.t("data.kpi.reports"), c.int(s.reports), Some(c.tf("data.kpi.reports_split", &[&c.int(bolagsverket_reports), &c.int(s.esef_reports)]))))
                (kpi(c.t("data.kpi.companies"), c.int(s.companies), None))
                (kpi(c.t("data.kpi.facts"), c.int(s.facts), Some(c.tf("data.kpi.facts_split", &[&c.int(s.numeric_facts), &c.int(s.facts - s.numeric_facts), &c.int(s.dimensional_facts)]))))
                (kpi(c.t("data.kpi.concepts"), c.int(s.concepts), None))
                @if let Some(r) = v.registry {
                    (kpi(c.t("data.kpi.registry"), c.int(r.distinct_orgnr), Some(c.tf("data.kpi.registry_rows", &[&c.int(r.rows), &c.int(r.active_rows)]))))
                }
                @if !s.newest_period.is_empty() {
                    (kpi(c.t("data.kpi.periods"), format!("{} → {}", s.oldest_period.get(..4).unwrap_or(""), s.newest_period.get(..4).unwrap_or("")), Some(c.tf("data.kpi.raw", &[&c.int(s.raw_files)]))))
                }
            }

            // Mapa de fuentes
            div.card.glass {
                h2.card-title { (c.t("data.sources")) (info_btn(c.t("data.sources"), c.t("data.sources_tip"))) }
                div.table-wrap.flat {
                    table {
                        caption.sr-only { (c.t("data.sources")) }
                        thead { tr {
                            th scope="col" { (c.t("data.col.source")) }
                            th.hide-sm scope="col" { (c.t("data.col.provides")) }
                            th scope="col" { (c.t("data.col.access")) }
                            th.hide-sm scope="col" { (c.t("data.col.refresh")) }
                        } }
                        tbody {
                            tr { td { a href="#s-bv-org" { (c.t("data.s1.title")) } } td.hide-sm { (c.t("data.s1.short")) } td { (c.t("data.access.oauth")) } td.hide-sm { (c.t("data.s1.refresh")) } }
                            tr { td { a href="#s-bv-docs" { (c.t("data.s2.title")) } } td.hide-sm { (c.t("data.s2.short")) } td { (c.t("data.access.oauth")) } td.hide-sm { (c.t("data.s2.refresh")) } }
                            tr { td { a href="#s-registry" { (c.t("data.s3.title")) } } td.hide-sm { (c.t("data.s3.short")) } td { (c.t("data.access.open")) } td.hide-sm { (c.t("data.s3.refresh")) } }
                            tr { td { a href="#s-scb" { (c.t("data.s4.title")) } } td.hide-sm { (c.t("data.s4.short")) } td { (c.t("data.access.open")) } td.hide-sm { (c.t("data.s4.refresh")) } }
                            tr { td { a href="#s-gleif" { (c.t("data.s5.title")) } } td.hide-sm { (c.t("data.s5.short")) } td { (c.t("data.access.open")) } td.hide-sm { (c.t("data.s5.refresh")) } }
                            tr { td { a href="#s-xbrl" { (c.t("data.s6.title")) } } td.hide-sm { (c.t("data.s6.short")) } td { (c.t("data.access.open")) } td.hide-sm { (c.t("data.s6.refresh")) } }
                            tr { td { a href="#s-tables" { (c.t("data.s7.title")) } } td.hide-sm { (c.t("data.s7.short")) } td { (c.t("data.access.internal")) } td.hide-sm { (c.t("data.s7.refresh")) } }
                        }
                    }
                }
                p.note { (c.t("data.flow")) }
            }

            (source_card(c, "s-bv-org", c.t("data.s1.title"), c.t("data.s1.about"),
                &[("data.meta.endpoint", "POST …/vardefulla-datamangder/v1/organisationer".into()), ("data.meta.body", "{ \"identitetsbeteckning\": \"<orgnr>\" }".into()), ("data.meta.limit", c.t("data.s1.limit").into())],
                BV_ORG, SAMPLE_BV_ORG, true))
            (source_card(c, "s-bv-docs", c.t("data.s2.title"), c.t("data.s2.about"),
                &[("data.meta.endpoint", "POST …/dokumentlista · GET …/dokument/{id}".into()), ("data.meta.format", "ZIP → XHTML (iXBRL) · se-gen-base:*".into()), ("data.meta.limit", c.t("data.s2.limit").into())],
                BV_DOCS, SAMPLE_BV_DOCS, false))
            (source_card(c, "s-registry", c.t("data.s3.title"), c.t("data.s3.about"),
                &[("data.meta.endpoint", "https://vardefulla-datamangder.bolagsverket.se/bolagsverket/bolagsverket_bulkfil.zip".into()), ("data.meta.format", "CSV (;) · sub-fields separated by $".into()), ("data.meta.stored", c.t("data.s3.stored").into())],
                REGISTRY_COLS, SAMPLE_REGISTRY, false))
            @if let Some(r) = v.registry {
                div.card.glass.data-registry {
                    h2.card-title { (c.t("data.registry_now")) }
                    p.muted { (c.tf("data.registry_meta", &[r.source.as_str(), r.imported_at.as_str()])) }
                    div.data-two {
                        div.table-wrap.flat { table {
                            caption.sr-only { (c.t("data.registry_types")) }
                            thead { tr { th scope="col" { (c.t("data.registry_types")) } th.right scope="col" { (c.t("data.col.rows")) } } }
                            tbody { @for (k, n) in &r.by_id_type { tr { td.mono { (k) } td.right.num { (c.int(*n)) } } } }
                        } }
                        div.table-wrap.flat { table {
                            caption.sr-only { (c.t("data.registry_forms")) }
                            thead { tr { th scope="col" { (c.t("data.registry_forms")) } th.right scope="col" { (c.t("data.col.rows")) } th.right.hide-sm scope="col" { (c.t("data.col.active")) } } }
                            tbody { @for (k, n, a) in r.by_form.iter().take(10) { tr { td.mono { (k) } td.right.num { (c.int(*n)) } td.right.num.hide-sm { (c.int(*a)) } } } }
                        } }
                    }
                }
            }
            (source_card(c, "s-scb", c.t("data.s4.title"), c.t("data.s4.about"),
                &[("data.meta.endpoint", "GET https://statistikdatabasen.scb.se/api/v2/tables/TAB1270/data".into()), ("data.meta.limit", c.t("data.s4.limit").into())],
                SCB, "SectorMedians { margin: 6.1, solidity: 38.0, liquidity: 142.0, year: \"2024\", sni_code: \"62.010\", size_class: \"10-19\", exact_sni: true, exact_size: true }", false))
            (source_card(c, "s-gleif", c.t("data.s5.title"), c.t("data.s5.about"),
                &[("data.meta.endpoint", "GET https://api.gleif.org/api/v1/lei-records".into()), ("data.meta.limit", c.t("data.s5.limit").into())],
                GLEIF, SAMPLE_GLEIF, false))
            (source_card(c, "s-xbrl", c.t("data.s6.title"), c.t("data.s6.about"),
                &[("data.meta.endpoint", "GET https://filings.xbrl.org/api/entities/{LEI}/filings".into()), ("data.meta.format", "xBRL-JSON · IFRS taxonomy (ifrs-full:*)".into()), ("data.meta.limit", c.t("data.s6.limit").into())],
                XBRL, SAMPLE_XBRL, false))

            div.card.glass #s-tables {
                h2.card-title { (c.t("data.s7.title")) }
                p.lead { (c.t("data.s7.about")) }
                div.data-two {
                    div { h3.card-subtitle { "report" } (field_table(c, "report", T_REPORT)) }
                    div { h3.card-subtitle { "fact" } (field_table(c, "fact", T_FACT)) }
                }
            }

            // Cómo se reparte lo guardado
            div.card.glass {
                h2.card-title { (c.t("data.shape")) (info_btn(c.t("data.shape"), c.t("data.shape_tip"))) }
                div.data-two {
                    div.table-wrap.flat { table {
                        caption.sr-only { (c.t("data.by_taxonomy")) }
                        thead { tr { th scope="col" { (c.t("data.by_taxonomy")) } th.right scope="col" { (c.t("data.col.facts")) } th.right scope="col" { (c.t("data.col.concepts")) } } }
                        tbody {
                            @for (p, n, d) in &s.by_taxonomy { tr { td.mono { (if p.is_empty() { "—" } else { p.as_str() }) } td.right.num { (c.int(*n)) } td.right.num { (c.int(*d)) } } }
                            @if s.by_taxonomy.is_empty() { tr { td colspan="3" { div.empty-state { strong { (c.t("data.empty")) } } } } }
                        }
                    } }
                    div.table-wrap.flat { table {
                        caption.sr-only { (c.t("data.by_year")) }
                        thead { tr { th scope="col" { (c.t("data.by_year")) } th.right scope="col" { "Bolagsverket" } th.right scope="col" { "ESEF" } } }
                        tbody { @for (y, b, e) in &s.by_year { tr { td.mono { (y) } td.right.num { (c.int(*b)) } td.right.num { (c.int(*e)) } } } }
                    } }
                    div.table-wrap.flat { table {
                        caption.sr-only { (c.t("data.by_unit")) }
                        thead { tr { th scope="col" { (c.t("data.by_unit")) } th.right scope="col" { (c.t("data.col.facts")) } } }
                        tbody { @for (u, n) in &s.by_unit { tr { td.mono { (u) } td.right.num { (c.int(*n)) } } } }
                    } }
                    div.table-wrap.flat { table {
                        caption.sr-only { (c.t("data.by_axis")) }
                        thead { tr { th scope="col" { (c.t("data.by_axis")) } th.right scope="col" { (c.t("data.col.facts")) } } }
                        tbody { @for (a, n) in s.axes.iter().take(15) { tr { td.mono.break-any { (a) } td.right.num { (c.int(*n)) } } } }
                    } }
                }
            }

            // Explorador de conceptos
            div.card.glass #explorer {
                h2.card-title { (c.t("data.explorer")) (info_btn(c.t("data.explorer"), c.t("data.explorer_tip"))) }
                form.inline-form method="get" action="/datos" {
                    input.text-input type="search" name="q" value=(v.q) placeholder=(c.t("data.search_ph")) aria-label=(c.t("data.search_ph"));
                    select.text-input name="tax" aria-label=(c.t("data.col.taxonomy")) {
                        option value="" selected[v.taxonomy.is_empty()] { (c.t("data.all_taxonomies")) }
                        @for (p, _, _) in s.by_taxonomy.iter().filter(|(p, _, _)| !p.is_empty()) { option value=(p) selected[v.taxonomy == p.as_str()] { (p) } }
                    }
                    button.btn type="submit" { (c.t("data.filter")) }
                }
                p.result-count role="status" { (c.tf("data.explorer_count", &[&c.int(v.total_concepts)])) }
                div.table-wrap {
                    table.data-concepts {
                        caption.sr-only { (c.t("data.explorer")) }
                        thead { tr {
                            th scope="col" { (c.t("data.col.concept")) }
                            th.right scope="col" { (c.t("data.col.facts")) }
                            th.right.hide-sm scope="col" { (c.t("data.col.reports")) }
                            th.right.hide-sm scope="col" { (c.t("data.col.companies")) }
                            th.right.hide-sm scope="col" { (c.t("data.col.dims")) }
                            th scope="col" { (c.t("data.col.example")) }
                            th.hide-sm scope="col" { (c.t("data.col.maps_to")) }
                        } }
                        tbody {
                            @for r in v.concepts {
                                @let members = mapped_members(&r.concept);
                                tr {
                                    td.mono.break-any { (r.concept) }
                                    td.right.num { (c.int(r.facts)) }
                                    td.right.num.hide-sm { (c.int(r.reports)) }
                                    td.right.num.hide-sm { (c.int(r.companies)) }
                                    td.right.num.hide-sm { (c.int(r.with_dims)) }
                                    td.data-example { (example_text(c, r)) @if !r.example_period.is_empty() { " " span.muted { (r.example_period) } } }
                                    td.hide-sm.mono { @if members.is_empty() { span.muted { "—" } } @else { (members.join(", ")) } }
                                }
                            }
                            @if v.concepts.is_empty() { tr { td colspan="7" { div.empty-state { strong { (c.t("data.empty")) } } } } }
                        }
                    }
                }
                @if pages > 1 {
                    nav.pager aria-label=(c.t("act.pager")) {
                        @if v.page > 1 { a.btn href=(page_url(v.page - 1)) { (c.t("act.prev")) } }
                        span.muted { (c.tf("act.page_of", &[&v.page.to_string(), &pages.to_string()])) }
                        @if v.page < pages { a.btn href=(page_url(v.page + 1)) { (c.t("act.next")) } }
                    }
                }
                p.note { (c.t("data.explorer_note")) }
            }
        },
    )
}
