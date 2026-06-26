//! core::sites — site lifecycle domain logic (PLATFORM-AGNOSTIC).
//!
//! Phase 1 task 1.2: create / list / get / delete persisted via `state::store`.
//! Later tasks extend `create` to also generate the docroot, cert, vhost, and
//! edge-router route (§7); this module stays the single entry point for site
//! operations so commands/ remain thin.

use crate::error::{Error, Result};
use crate::state::models::{NewSite, ServiceStatus, Site};
use crate::state::store;
use rusqlite::Connection;
use uuid::Uuid;

/// Create a site: assign an id, default to stopped + SSL on, persist, return it.
/// Fails if the domain is already in use.
pub fn create(conn: &Connection, new: NewSite) -> Result<Site> {
    if store::domain_exists(conn, &new.domain)? {
        return Err(Error::Other(format!(
            "domain already in use: {}",
            new.domain
        )));
    }
    let site = Site {
        id: Uuid::new_v4().to_string(),
        name: new.name,
        domain: new.domain,
        site_type: new.site_type,
        status: ServiceStatus::Stopped,
        php_version: new.php_version,
        web_server: new.web_server,
        ssl: true,
        path: new.path,
        created_at: store::db_now(conn)?,
    };
    store::insert_site(conn, &site)?;
    Ok(site)
}

/// All sites, newest first.
pub fn list(conn: &Connection) -> Result<Vec<Site>> {
    store::list_sites(conn)
}

/// One site by id, or `None`.
pub fn get(conn: &Connection, id: &str) -> Result<Option<Site>> {
    store::get_site(conn, id)
}

/// Delete a site by id; returns whether it existed.
pub fn delete(conn: &Connection, id: &str) -> Result<bool> {
    store::delete_site(conn, id)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::state::db;
    use crate::state::models::{SiteType, WebServer};

    fn sample(name: &str, domain: &str) -> NewSite {
        NewSite {
            name: name.into(),
            domain: domain.into(),
            site_type: SiteType::Wordpress,
            php_version: "8.3".into(),
            web_server: WebServer::Nginx,
            path: format!("~/Sites/{name}"),
        }
    }

    #[test]
    fn create_then_list_round_trips() {
        let conn = db::open_in_memory().unwrap();
        let created = create(&conn, sample("Acme", "acme.test")).unwrap();

        assert!(!created.id.is_empty());
        assert!(matches!(created.status, ServiceStatus::Stopped));
        assert!(created.ssl);
        assert!(!created.created_at.is_empty());

        let all = list(&conn).unwrap();
        assert_eq!(all.len(), 1);
        let s = &all[0];
        assert_eq!(s.id, created.id);
        assert_eq!(s.name, "Acme");
        assert_eq!(s.domain, "acme.test");
        assert!(matches!(s.site_type, SiteType::Wordpress));
        assert!(matches!(s.web_server, WebServer::Nginx));
        assert_eq!(s.php_version, "8.3");
    }

    #[test]
    fn duplicate_domain_is_rejected() {
        let conn = db::open_in_memory().unwrap();
        create(&conn, sample("One", "dup.test")).unwrap();
        let err = create(&conn, sample("Two", "dup.test")).unwrap_err();
        assert!(err.to_string().contains("domain already in use"));
        assert_eq!(list(&conn).unwrap().len(), 1);
    }

    #[test]
    fn get_and_delete() {
        let conn = db::open_in_memory().unwrap();
        let site = create(&conn, sample("Blog", "blog.test")).unwrap();

        let fetched = get(&conn, &site.id).unwrap().expect("site exists");
        assert_eq!(fetched.domain, "blog.test");

        assert!(delete(&conn, &site.id).unwrap());
        assert!(get(&conn, &site.id).unwrap().is_none());
        assert!(!delete(&conn, &site.id).unwrap(), "second delete is a no-op");
    }

    #[test]
    fn enum_values_persist_as_canonical_strings() {
        let conn = db::open_in_memory().unwrap();
        let mut new = sample("LO", "lo.test");
        new.site_type = SiteType::Php;
        new.web_server = WebServer::Openlitespeed;
        create(&conn, new).unwrap();

        // Read the raw TEXT to confirm DB storage matches the wire format.
        let (t, ws): (String, String) = conn
            .query_row(
                "SELECT type, web_server FROM sites WHERE domain='lo.test'",
                [],
                |r| Ok((r.get(0)?, r.get(1)?)),
            )
            .unwrap();
        assert_eq!(t, "php");
        assert_eq!(ws, "openlitespeed");
    }
}
