//! Anonymise personal data on a pull (plan §11 Q6, S3 — built 15 Oct 2026). A pull
//! copies a live site's customers, commenters and users onto a laptop; with this ON
//! for a site, the STAGING copy is scrubbed before the swap, so the local database
//! never holds the live values at all — not even in `_prepull`.
//!
//! One rule per table, by its suffix (the live prefix is put in front), as plain SQL
//! over the staging database: e-mails become `user<id>@example.invalid`, names and
//! addresses empty strings, IPs `0.0.0.0`, agents empty; sessions and form entries —
//! whose VALUES are the personal data — are emptied. Idempotent, so every pull applies
//! every rule to every table in the copy (a table live did not touch was copied from
//! the local copy, which may predate the switch).
//!
//! What a rule touched is recorded in the base (`SyncBase::anonymised`), and a push
//! REFUSES those tables (`push::push_from`): the scrubbed copy must never land on live.

use super::pull::LocalSite;
use crate::error::Result;

/// One table's rule: the bare table name (without the site's prefix) and the SQL
/// statements to run over it, `{t}` standing for the quoted `db`.`table`.
pub struct Rule {
    pub table: &'static str,
    pub what: &'static str,
    pub sql: &'static [&'static str],
}

/// The rules, in ONE place. A plugin's table is one row here.
pub const RULES: &[Rule] = &[
    Rule {
        table: "users",
        what: "e-mails, URLs and display names",
        sql: &["UPDATE {t} SET user_email = CONCAT('user', ID, '@example.invalid'), user_url = '', display_name = CONCAT('User ', ID), user_activation_key = ''"],
    },
    Rule {
        table: "usermeta",
        what: "names, descriptions, billing and shipping details, session tokens",
        sql: &[
            "UPDATE {t} SET meta_value = '' WHERE meta_key IN ('first_name', 'last_name', 'nickname', 'description') OR meta_key LIKE 'billing\\_%' OR meta_key LIKE 'shipping\\_%'",
            "DELETE FROM {t} WHERE meta_key = 'session_tokens'",
        ],
    },
    Rule {
        table: "comments",
        what: "commenter names, e-mails, URLs, IPs and agents",
        sql: &["UPDATE {t} SET comment_author = CONCAT('Commenter ', comment_ID), comment_author_email = CONCAT('commenter', comment_ID, '@example.invalid'), comment_author_url = '', comment_author_IP = '0.0.0.0', comment_agent = ''"],
    },
    Rule {
        table: "postmeta",
        what: "WooCommerce order billing and shipping details (legacy order storage)",
        sql: &[
            "UPDATE {t} SET meta_value = CONCAT('order', post_id, '@example.invalid') WHERE meta_key = '_billing_email'",
            "UPDATE {t} SET meta_value = '' WHERE meta_key IN ('_billing_first_name', '_billing_last_name', '_billing_company', '_billing_address_1', '_billing_address_2', '_billing_city', '_billing_postcode', '_billing_phone', '_shipping_first_name', '_shipping_last_name', '_shipping_company', '_shipping_address_1', '_shipping_address_2', '_shipping_city', '_shipping_postcode', '_shipping_phone', '_customer_ip_address', '_customer_user_agent')",
        ],
    },
    Rule {
        table: "wc_orders",
        what: "WooCommerce order e-mails, IPs, agents and notes",
        sql: &["UPDATE {t} SET billing_email = CONCAT('order', id, '@example.invalid'), ip_address = '0.0.0.0', user_agent = '', customer_note = ''"],
    },
    Rule {
        table: "wc_order_addresses",
        what: "WooCommerce order addresses",
        sql: &["UPDATE {t} SET first_name = '', last_name = '', company = '', address_1 = '', address_2 = '', city = '', postcode = '', phone = '', email = CONCAT('order', order_id, '@example.invalid')"],
    },
    Rule {
        table: "wc_customer_lookup",
        what: "WooCommerce customer names and e-mails",
        sql: &["UPDATE {t} SET username = CONCAT('user', customer_id), first_name = '', last_name = '', email = CONCAT('user', customer_id, '@example.invalid')"],
    },
    Rule { table: "woocommerce_sessions", what: "WooCommerce sessions (carts carry addresses)", sql: &["DELETE FROM {t}"] },
    Rule { table: "gf_entry", what: "Gravity Forms entries (their fields ARE the personal data)", sql: &["DELETE FROM {t}"] },
    Rule { table: "gf_entry_meta", what: "Gravity Forms entry fields", sql: &["DELETE FROM {t}"] },
    Rule { table: "gf_entry_notes", what: "Gravity Forms entry notes", sql: &["DELETE FROM {t}"] },
    Rule { table: "wpforms_entries", what: "WPForms entries (their fields ARE the personal data)", sql: &["DELETE FROM {t}"] },
    Rule { table: "wpforms_entry_fields", what: "WPForms entry fields", sql: &["DELETE FROM {t}"] },
    Rule { table: "wpforms_entry_meta", what: "WPForms entry meta", sql: &["DELETE FROM {t}"] },
];

fn q(name: &str) -> String {
    format!("`{}`", name.replace('`', "``"))
}

/// The statements to run over `db` for the tables of `present` that a rule covers:
/// `(full table name, what, sql)`. Pure.
pub fn statements(prefix: &str, db: &str, present: &[String]) -> Vec<(String, &'static str, String)> {
    let mut out = Vec::new();
    // Every table whose name without the prefix — and, on a network, without a sub-site's
    // `<id>_` — is a rule's: `wp_2_comments` gets the comments rule (S5).
    for r in RULES {
        for full in present.iter().filter(|t| t.starts_with(prefix) && super::push::bare_name(prefix, t) == r.table) {
            let t = format!("{}.{}", q(db), q(full));
            for s in r.sql {
                out.push((full.clone(), r.what, s.replace("{t}", &t)));
            }
        }
    }
    out
}

/// Run every rule over the tables of `db` (the staging copy). Answers the tables a
/// rule touched; a statement the database refuses (a column this site's schema does
/// not have) is named on a line and skipped — a column that is not there holds nothing.
pub fn apply(local: &LocalSite<'_>, db: &str, prefix: &str, on_line: &mut (dyn FnMut(&str) + Send)) -> Result<Vec<String>> {
    let present = local.engine.list_tables(local.client, local.port, db)?;
    let mut touched: Vec<String> = Vec::new();
    for (table, what, sql) in statements(prefix, db, &present) {
        match crate::core::database::mysql_exec(local.client, local.port, &sql, &format!("anonymising {table}")) {
            Ok(()) => {
                if !touched.contains(&table) {
                    touched.push(table.clone());
                    on_line(&format!("  anonymised {table} — {what}"));
                }
            }
            Err(e) => on_line(&format!("  {table}: a rule did not apply here ({}) — skipped", e.to_string().lines().last().unwrap_or("").trim())),
        }
    }
    Ok(touched)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// **Every rule covers one table by its bare name, with the site's prefix put in
    /// front, and only tables the copy HAS are touched** (ledger #879). Plant: match on
    /// `ends_with` instead of the full name → `wp_notusers` would get the users rule.
    #[test]
    fn rules_apply_by_prefixed_name_to_present_tables_only() {
        let present: Vec<String> = ["wpx_users", "wpx_posts", "wpx_comments", "wp_users", "wpx_notusers", "wpx_2_comments", "wpx_2_posts"].iter().map(|s| s.to_string()).collect();
        let s = statements("wpx_", "site_pull", &present);
        let tables: Vec<&str> = s.iter().map(|(t, _, _)| t.as_str()).collect();
        assert_eq!(tables, ["wpx_users", "wpx_comments", "wpx_2_comments"], "{tables:?}");
        assert!(s[0].2.starts_with("UPDATE `site_pull`.`wpx_users` SET user_email = CONCAT('user', ID, '@example.invalid')"), "{}", s[0].2);
        assert!(s[1].2.contains("comment_author_IP = '0.0.0.0'"));
        // Every rule names at least one statement, and none keeps the live value by accident.
        for r in RULES {
            assert!(!r.sql.is_empty() && r.sql.iter().all(|s| s.contains("{t}")), "{}", r.table);
        }
        assert!(RULES.iter().any(|r| r.table == "wc_orders") && RULES.iter().any(|r| r.table == "wpforms_entries"));
    }
}
