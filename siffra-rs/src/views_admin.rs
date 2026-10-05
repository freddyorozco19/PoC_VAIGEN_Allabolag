//! Pantallas de cuentas: inicio de sesión, perfil, CRUD de usuarios y registro de actividad.

use maud::{html, Markup};

use crate::auth;
use crate::db::{ActivityFilter, ActivityRow, Kpis, Role, User, UserSummary};
use crate::i18n::Lang;
use crate::util::urlencode;
use crate::views::{auth_layout, brand_mark_pub, icon, icon_sized, info_btn, layout, role_pill, Ctx};

// ───────────────────────── Inicio de sesión ─────────────────────────

pub fn login_page(c: &Ctx, identifier: &str, next: &str, error: Option<&str>, pre_csrf: &str) -> Markup {
    auth_layout(
        c,
        c.t("auth.login"),
        html! {
            div.auth-card.glass {
                div.auth-brand { (brand_mark_pub()) span.brand-word { "Sif" span.accent { "f" } "ra" } }
                h1.auth-title { (c.t("auth.welcome")) }
                p.lead { (c.t("auth.subtitle")) }
                @if let Some(key) = error {
                    p.form-error role="alert" { (icon_sized("octagon-alert", "icon-sm")) (c.t(key)) }
                }
                form.stack-form method="post" action="/login" {
                    input type="hidden" name="csrf" value=(pre_csrf);
                    input type="hidden" name="next" value=(next);
                    label.field-label for="identifier" { (c.t("auth.identifier")) }
                    input #identifier.text-input type="text" name="identifier" value=(identifier) required autocomplete="username" autofocus
                        autocapitalize="none" spellcheck="false";
                    label.field-label for="password" { (c.t("auth.password")) }
                    input #password.text-input type="password" name="password" required autocomplete="current-password";
                    button.btn.btn-primary.btn-block type="submit" { (icon_sized("lock", "icon-sm")) (c.t("auth.submit")) }
                }
                p.note { (c.t("auth.no_account")) }
            }
        },
    )
}

// ───────────────────────── Perfil propio ─────────────────────────

pub struct Notice<'a> {
    pub ok: bool,
    pub text: &'a str,
}

fn notice(n: Option<&Notice>) -> Markup {
    html! {
        @if let Some(n) = n {
            @if n.ok {
                p.form-ok role="status" { (icon_sized("check-circle", "icon-sm")) (n.text) }
            } @else {
                p.form-error role="alert" { (icon_sized("octagon-alert", "icon-sm")) (n.text) }
            }
        }
    }
}

fn lang_select(name: &str, current: Lang) -> Markup {
    html! {
        select.text-input name=(name) id=(name) {
            @for l in Lang::ALL { option value=(l.code()) selected[l == current] { (l.name()) } }
        }
    }
}

pub fn profile_page(c: &Ctx, user: &User, force: bool, profile_notice: Option<&Notice>, password_notice: Option<&Notice>) -> Markup {
    layout(
        c,
        c.t("prof.title"),
        html! {
            div.page-head { h1.page-title { (c.t("prof.title")) } }
            @if force {
                p.form-error role="alert" { (icon_sized("lock", "icon-sm")) (c.t("prof.force_change")) }
            }
            div.grid-2 {
                div.card.glass {
                    h2.card-title { (c.t("prof.data")) }
                    (notice(profile_notice))
                    form.stack-form method="post" action="/profile" {
                        (c.csrf_input())
                        label.field-label for="name" { (c.t("usr.field.name")) }
                        input #name.text-input type="text" name="name" value=(user.name) required maxlength="80";
                        label.field-label for="lang" { (c.t("usr.field.lang")) }
                        (lang_select("lang", user.lang))
                        button.btn.btn-primary type="submit" { (c.t("common.save")) }
                    }
                    dl.facts.facts-tight {
                        dt { (c.t("usr.field.login")) } dd { (user.label()) }
                        dt { (c.t("usr.col.role")) } dd { (role_pill(c, user.role)) }
                        dt { (c.t("usr.col.last_login")) } dd { @if let Some(t) = &user.last_login { (time_tag(t)) } @else { "—" } }
                        dt { (c.t("usr.col.created")) } dd { (time_tag(&user.created_at)) }
                    }
                }
                div.card.glass {
                    h2.card-title { (c.t("prof.password")) }
                    (notice(password_notice))
                    form.stack-form method="post" action="/profile/password" {
                        (c.csrf_input())
                        label.field-label for="current" { (c.t("prof.current_password")) }
                        input #current.text-input type="password" name="current" required autocomplete="current-password";
                        label.field-label for="new1" { (c.t("prof.new_password")) }
                        input #new1.text-input type="password" name="new1" required minlength=(auth::MIN_PASSWORD) autocomplete="new-password";
                        p.field-hint { (c.tf("pw.hint", &[&auth::MIN_PASSWORD.to_string()])) }
                        label.field-label for="new2" { (c.t("prof.repeat_password")) }
                        input #new2.text-input type="password" name="new2" required minlength=(auth::MIN_PASSWORD) autocomplete="new-password";
                        button.btn.btn-primary type="submit" { (icon_sized("key", "icon-sm")) (c.t("prof.change_password")) }
                    }
                }
            }
        },
    )
}

/// Hora en UTC con atributo `datetime`; el navegador la convierte a su zona horaria.
pub fn time_tag(iso: &str) -> Markup {
    let shown = iso.replace('T', " ").trim_end_matches('Z').to_string() + " UTC";
    html! { time datetime=(iso) { (shown) } }
}

// ───────────────────────── Usuarios (CRUD) ─────────────────────────

pub fn flash_text(key: &str) -> Option<&'static str> {
    Some(match key {
        "created" => "usr.flash.created",
        "updated" => "usr.flash.updated",
        "deleted" => "usr.flash.deleted",
        "password" => "usr.flash.password",
        _ => return None,
    })
}

pub fn users_page(c: &Ctx, actor: &User, users: &[User], q: &str, role: Option<Role>, flash: Option<&str>, active_superadmins: i64) -> Markup {
    layout(
        c,
        c.t("nav.users"),
        html! {
            div.page-head.page-head-row {
                div {
                    h1.page-title { (c.t("nav.users")) }
                    p.lead { (c.t(if actor.role == Role::Superadmin { "usr.lead.super" } else { "usr.lead.admin" })) }
                }
                a.btn.btn-primary href="/users/new" { (icon_sized("plus", "icon-sm")) (c.t("usr.new")) }
            }
            @if let Some(key) = flash.and_then(flash_text) {
                p.form-ok role="status" { (icon_sized("check-circle", "icon-sm")) (c.t(key)) }
            }
            form.filters method="get" action="/users" role="search" {
                div.search-field {
                    (icon("search"))
                    input.search-input type="search" name="q" value=(q) placeholder=(c.t("usr.search")) aria-label=(c.t("usr.search"));
                }
                select.text-input name="role" aria-label=(c.t("usr.col.role")) {
                    option value="" { (c.t("usr.all_roles")) }
                    @for r in Role::ALL { option value=(r.code()) selected[Some(r) == role] { (c.t(r.label_key())) } }
                }
                button.btn type="submit" { (c.t("usr.filter")) }
            }
            p.result-count role="status" { (c.tf(if users.len() == 1 { "usr.count.one" } else { "usr.count.many" }, &[&users.len().to_string()])) }
            div.table-wrap {
                table.row-link {
                    caption.sr-only { (c.t("nav.users")) }
                    thead { tr {
                        th scope="col" { (c.t("usr.col.user")) }
                        th scope="col" { (c.t("usr.col.role")) }
                        th scope="col" { (c.t("usr.col.status")) }
                        th.hide-sm scope="col" { (c.t("usr.field.lang")) }
                        th.hide-sm scope="col" { (c.t("usr.col.last_login")) }
                        th.right scope="col" { (c.t("usr.col.actions")) }
                    } }
                    tbody {
                        @for u in users {
                            @let manage = auth::can_manage(actor, u);
                            tr {
                                td.top {
                                    div.user-cell {
                                        span.avatar { (u.initials()) }
                                        div { strong { (u.name) } @if u.id == actor.id { " " span.pill.pill-user { (c.t("usr.you")) } } div.org-sub { (u.label()) } }
                                    }
                                }
                                td.top { (role_pill(c, u.role)) }
                                td.top { span class={"pill " (if u.active { "pill-good" } else { "pill-bad" })} { span.pill-dot aria-hidden="true" {} (c.t(if u.active { "usr.active" } else { "usr.disabled" })) } }
                                td.top.hide-sm { (u.lang.name()) }
                                td.top.hide-sm { @if let Some(t) = &u.last_login { (time_tag(t)) } @else { span.muted { "—" } } }
                                td.top.right {
                                    div.row-actions {
                                        @if manage {
                                            a.icon-link href=(format!("/users/{}/edit", u.id)) data-tip=(c.t("usr.edit")) aria-label=(c.tf("usr.edit_aria", &[&u.name])) { (icon("edit")) }
                                            @if auth::can_delete(actor, u, active_superadmins) {
                                                a.icon-link.danger href=(format!("/users/{}/delete", u.id)) data-tip=(c.t("usr.delete")) aria-label=(c.tf("usr.delete_aria", &[&u.name])) { (icon("trash")) }
                                            }
                                        } @else { span.muted { "—" } }
                                    }
                                }
                            }
                        }
                        @if users.is_empty() {
                            tr { td colspan="6" { div.empty-state { strong { (c.t("usr.empty")) } } } }
                        }
                    }
                }
            }
            @if actor.role == Role::Admin { p.note { (c.t("usr.admin_note")) } }
        },
    )
}

/// Datos del formulario de usuario (para repintarlo con el error y sin perder lo escrito).
#[derive(Default, Clone)]
pub struct UserFormData {
    pub name: String,
    pub email: String,
    pub username: String,
    pub role: String,
    pub active: bool,
    pub lang: String,
    pub must_change: bool,
    pub password: String,
}

pub fn user_form_page(c: &Ctx, actor: &User, target: Option<&User>, data: &UserFormData, error: Option<&str>) -> Markup {
    let editing = target.is_some();
    let roles = auth::assignable_roles(actor);
    let current_role = Role::from_code(&data.role).unwrap_or(Role::User);
    let current_lang = Lang::from_code(&data.lang).unwrap_or(Lang::DEFAULT);
    let action = match target {
        Some(t) => format!("/users/{}", t.id),
        None => "/users".to_string(),
    };
    // Una cuenta no se desactiva ni se cambia de rol a sí misma.
    let self_edit = target.is_some_and(|t| t.id == actor.id);
    layout(
        c,
        c.t(if editing { "usr.edit_title" } else { "usr.new" }),
        html! {
            a.back href="/users" { (icon_sized("arrow-left", "icon-sm")) (c.t("usr.back")) }
            div.page-head { h1.page-title { (c.t(if editing { "usr.edit_title" } else { "usr.new" })) } }
            div.card.glass.narrow {
                @if let Some(key) = error { p.form-error role="alert" { (icon_sized("octagon-alert", "icon-sm")) (c.t(key)) } }
                form.stack-form method="post" action=(action) {
                    (c.csrf_input())
                    label.field-label for="name" { (c.t("usr.field.name")) }
                    input #name.text-input type="text" name="name" value=(data.name) required maxlength="80";
                    label.field-label for="email" { (c.t("usr.field.email")) }
                    input #email.text-input type="email" name="email" value=(data.email) maxlength="120" autocomplete="off" autocapitalize="none";
                    @if actor.role == Role::Superadmin {
                        label.field-label for="username" { (c.t("usr.field.username")) }
                        input #username.text-input type="text" name="username" value=(data.username) maxlength="40" autocomplete="off" autocapitalize="none";
                        p.field-hint { (c.t("usr.field.login_hint")) }
                    }
                    label.field-label for="role" { (c.t("usr.col.role")) }
                    @if self_edit {
                        input type="hidden" name="role" value=(current_role.code());
                        p { (role_pill(c, current_role)) " " span.muted { (c.t("usr.err.own_role_status")) } }
                    } @else {
                        select.text-input name="role" id="role" {
                            @for r in roles.iter() { option value=(r.code()) selected[*r == current_role] { (c.t(r.label_key())) } }
                        }
                    }
                    label.field-label for="lang" { (c.t("usr.field.lang")) }
                    (lang_select("lang", current_lang))
                    @if editing {
                        @if self_edit {
                            input type="hidden" name="active" value="1";
                        } @else {
                            label.check { input type="checkbox" name="active" value="1" checked[data.active]; (c.t("usr.field.active")) }
                        }
                    }
                    @if !editing {
                        label.field-label for="password" { (c.t("usr.field.password")) }
                        input #password.text-input type="text" name="password" value=(data.password) autocomplete="off" minlength=(auth::MIN_PASSWORD);
                        p.field-hint { (c.t("usr.field.password_hint")) }
                        label.check { input type="checkbox" name="must_change" value="1" checked[data.must_change]; (c.t("usr.field.must_change")) }
                    }
                    div.form-actions {
                        button.btn.btn-primary type="submit" { (c.t("common.save")) }
                        a.btn href="/users" { (c.t("common.cancel")) }
                    }
                }
                @if let Some(t) = target {
                    @if !self_edit {
                        form.inline-form method="post" action=(format!("/users/{}/reset-password", t.id)) {
                            (c.csrf_input())
                            h2.card-title { (c.t("usr.reset.title")) }
                            p.muted { (c.t("usr.reset.text")) }
                            button.btn type="submit" { (icon_sized("key", "icon-sm")) (c.t("usr.reset.button")) }
                        }
                    }
                }
            }
        },
    )
}

pub fn user_delete_page(c: &Ctx, target: &User) -> Markup {
    layout(
        c,
        c.t("usr.delete_title"),
        html! {
            a.back href="/users" { (icon_sized("arrow-left", "icon-sm")) (c.t("usr.back")) }
            div.page-head { h1.page-title { (c.t("usr.delete_title")) } }
            div.card.glass.narrow {
                p { (c.tf("usr.delete_text", &[&target.name, &target.label()])) }
                p.muted { (c.t("usr.delete_history")) }
                form.form-actions method="post" action=(format!("/users/{}/delete", target.id)) {
                    (c.csrf_input())
                    button.btn.btn-danger type="submit" { (icon_sized("trash", "icon-sm")) (c.t("usr.delete_confirm")) }
                    a.btn href="/users" { (c.t("common.cancel")) }
                }
            }
        },
    )
}

/// Se muestra una sola vez tras crear una cuenta o restablecer su contraseña.
pub fn user_password_page(c: &Ctx, target: &User, password: &str, created: bool) -> Markup {
    layout(
        c,
        c.t("usr.temp.title"),
        html! {
            div.page-head { h1.page-title { (c.t("usr.temp.title")) } }
            div.card.glass.narrow {
                p.form-ok role="status" { (icon_sized("check-circle", "icon-sm")) (c.tf(if created { "usr.temp.created" } else { "usr.temp.reset" }, &[&target.name])) }
                dl.facts.facts-tight {
                    dt { (c.t("usr.field.login")) } dd { (target.label()) }
                    dt { (c.t("usr.temp.password")) }
                    dd { span.temp-password.mono { (password) } button.copy-btn.visible type="button" data-copy=(password) data-tip=(c.t("usr.temp.copy")) aria-label=(c.t("usr.temp.copy")) {
                        span.icon-copy { (icon_sized("copy", "icon-sm")) } span.icon-check { (icon_sized("check", "icon-sm")) } } }
                }
                p.form-warn { (icon_sized("triangle-alert", "icon-sm")) (c.t("usr.temp.once")) }
                @if target.must_change { p.muted { (c.t("usr.temp.must_change")) } }
                a.btn.btn-primary href="/users?ok=password" { (c.t("usr.back")) }
            }
        },
    )
}

// ───────────────────────── Actividad ─────────────────────────

pub struct ActivityView<'a> {
    pub filter: &'a ActivityFilter,
    pub rows: &'a [ActivityRow],
    pub total: i64,
    pub page: i64,
    pub per_page: i64,
    pub kpis: &'a Kpis,
    pub summaries: &'a [UserSummary],
    pub events: &'a [String],
    pub users: &'a [User],
}

pub fn event_label(c: &Ctx, event: &str) -> String {
    let key = format!("act.event.{event}");
    if crate::i18n::has_key(&key) { c.t(&key).to_string() } else { event.to_string() }
}

fn filter_query(f: &ActivityFilter) -> String {
    let mut parts: Vec<String> = Vec::new();
    if let Some(u) = f.user_id {
        parts.push(format!("user={u}"));
    }
    for (k, v) in [("event", &f.event), ("q", &f.q), ("from", &f.from), ("to", &f.to)] {
        if let Some(v) = v.as_ref().filter(|v| !v.is_empty()) {
            parts.push(format!("{k}={}", urlencode(v)));
        }
    }
    parts.join("&")
}

fn event_pill(c: &Ctx, event: &str) -> Markup {
    let class = match event {
        "login_ok" | "logout" => "pill-good",
        "login_fail" | "login_blocked" | "access_denied" => "pill-bad",
        "user_create" | "user_update" | "user_delete" | "password_reset" | "password_change" | "profile_update" => "pill-warn",
        _ => "pill-user",
    };
    html! { span class={"pill " (class)} { (event_label(c, event)) } }
}

pub fn activity_page(c: &Ctx, v: &ActivityView) -> Markup {
    let pages = ((v.total + v.per_page - 1) / v.per_page).max(1);
    let fq = filter_query(v.filter);
    let page_url = |p: i64| if fq.is_empty() { format!("/activity?page={p}") } else { format!("/activity?{fq}&page={p}") };
    layout(
        c,
        c.t("nav.activity"),
        html! {
            div.page-head.page-head-row {
                div {
                    h1.page-title { (c.t("nav.activity")) }
                    p.lead { (c.t("act.lead")) }
                }
                a.btn href=(if fq.is_empty() { "/activity.csv".to_string() } else { format!("/activity.csv?{fq}") }) { (icon_sized("download", "icon-sm")) (c.t("act.export")) }
            }
            dl.stats {
                div.stat { dt { (c.t("act.kpi.users")) } dd { (c.int(v.kpis.total_users)) } }
                div.stat { dt { (c.t("act.kpi.active")) } dd { (c.int(v.kpis.active_users_24h)) } }
                div.stat { dt { (c.t("act.kpi.events")) } dd { (c.int(v.kpis.events_24h)) } }
                div.stat { dt { (c.t("act.kpi.searches")) } dd { (c.int(v.kpis.searches_24h)) } }
                div.stat { dt { (c.t("act.kpi.failed")) } dd.neg[v.kpis.failed_logins_24h > 0] { (c.int(v.kpis.failed_logins_24h)) } }
            }

            div.card.glass {
                h2.card-title { (c.t("act.by_user")) (info_btn(c.t("act.by_user_about"), c.t("act.by_user_tip"))) }
                div.table-wrap.flat {
                    table {
                        caption.sr-only { (c.t("act.by_user")) }
                        thead { tr {
                            th scope="col" { (c.t("usr.col.user")) }
                            th scope="col" { (c.t("usr.col.role")) }
                            th.hide-sm scope="col" { (c.t("act.col.last_seen")) }
                            th.right scope="col" { (c.t("act.col.logins")) }
                            th.right.hide-sm scope="col" { (c.t("act.col.views")) }
                            th.right scope="col" { (c.t("act.col.searches")) }
                            th.right.hide-sm scope="col" { (c.t("act.col.companies")) }
                            th.right scope="col" { (c.t("act.col.detail")) }
                        } }
                        tbody {
                            @for s in v.summaries {
                                tr {
                                    td { strong { (s.name) } div.org-sub { (s.label) } }
                                    td { (role_pill(c, s.role)) }
                                    td.hide-sm { @if let Some(t) = s.last_seen.as_ref().or(s.last_login.as_ref()) { (time_tag(t)) } @else { span.muted { (c.t("act.never")) } } }
                                    td.right.num { (c.int(s.logins)) }
                                    td.right.num.hide-sm { (c.int(s.views)) }
                                    td.right.num { (c.int(s.searches)) }
                                    td.right.num.hide-sm { (c.int(s.companies)) }
                                    td.right { a.icon-link href=(format!("/activity?user={}", s.user_id)) data-tip=(c.t("act.see_user")) aria-label=(c.tf("act.see_user_aria", &[&s.name])) { (icon("activity")) } }
                                }
                            }
                        }
                    }
                }
            }

            form.filters.filters-wide method="get" action="/activity" role="search" {
                select.text-input name="user" aria-label=(c.t("usr.col.user")) {
                    option value="" { (c.t("act.all_users")) }
                    @for u in v.users { option value=(u.id) selected[v.filter.user_id == Some(u.id)] { (u.name) " — " (u.label()) } }
                }
                select.text-input name="event" aria-label=(c.t("act.col.event")) {
                    option value="" { (c.t("act.all_events")) }
                    @for e in v.events { option value=(e) selected[v.filter.event.as_deref() == Some(e.as_str())] { (event_label(c, e)) } }
                }
                input.text-input type="search" name="q" value=(v.filter.q.clone().unwrap_or_default()) placeholder=(c.t("act.search")) aria-label=(c.t("act.search"));
                label.date-field { span { (c.t("act.from")) } input.text-input type="date" name="from" value=(v.filter.from.clone().unwrap_or_default()); }
                label.date-field { span { (c.t("act.to")) } input.text-input type="date" name="to" value=(v.filter.to.clone().unwrap_or_default()); }
                button.btn.btn-primary type="submit" { (c.t("usr.filter")) }
                @if !fq.is_empty() { a.btn href="/activity" { (c.t("sok.clear")) } }
            }
            p.result-count role="status" { (c.tf("act.count", &[&c.int(v.total)])) }
            div.table-wrap {
                table.activity-table {
                    caption.sr-only { (c.t("nav.activity")) }
                    thead { tr {
                        th scope="col" { (c.t("act.col.time")) }
                        th scope="col" { (c.t("usr.col.user")) }
                        th scope="col" { (c.t("act.col.event")) }
                        th scope="col" { (c.t("act.col.detail")) }
                        th.hide-sm scope="col" { "IP" }
                    } }
                    tbody {
                        @for r in v.rows {
                            tr {
                                td.nowrap { (time_tag(&r.ts)) }
                                td { @if r.user_label.is_empty() { span.muted { "—" } } @else { (r.user_label) } }
                                td { (event_pill(c, &r.event)) }
                                td.detail { span.mono { (r.method) " " (r.path) @if !r.query.is_empty() { "?" (r.query) } } @if !r.detail.is_empty() { div.detail-text { (r.detail) } } }
                                td.hide-sm.mono data-tip=(r.ua) { (r.ip) }
                            }
                        }
                        @if v.rows.is_empty() { tr { td colspan="5" { div.empty-state { strong { (c.t("act.empty")) } } } } }
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
            p.note { (c.t("act.privacy")) }
        },
    )
}
