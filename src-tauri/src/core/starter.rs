//! The Blank-PHP starter: the page a new PHP site serves, and the one table it
//! reads from.
//!
//! WHY this replaced `<?php phpinfo();`. A phpinfo dump answers a question
//! nobody asked on their first page load ("what is in this PHP build?") and
//! answers none of the ones they did: is the stack actually wired, where do my
//! files live, how do I talk to a database. It also made the very first thing
//! rexenv shows a developer a wall of grey tables with somebody else's branding.
//!
//! Two files, both GENERATED ONCE at create time and never touched again:
//! - `index.php` — the page. Static; it decides what to render by looking for
//!   `db.php` beside it, so there is one template rather than two that drift.
//! - `db.php` — the connection, written ONLY when the site asked for a starter
//!   database. It carries the substituted credentials and returns a live PDO.
//!
//! Neither is ever overwritten. A retry re-enters this code with the user's
//! edited page already on disk, and "provisioning finished the job" must never
//! mean "provisioning reverted my work".

use std::path::Path;

use crate::core::database;
use crate::core::db::SqlClient;
use crate::error::Result;

/// The seeded table's name. One constant: it is substituted into `db.php`, read
/// back by `index.php` through `REXENV_DB['table']`, and named in the SQL below.
pub const TABLE: &str = "starter_items";

/// The connection `db.php` is generated with — the local-dev credentials every
/// rexenv engine runs with (loopback, root, no password).
#[derive(Debug, Clone)]
pub struct StarterDb {
    /// Display name of the engine, for the page and the generated comment.
    pub engine: &'static str,
    pub host: String,
    pub port: u16,
    pub database: String,
    pub username: String,
    pub password: String,
}

impl StarterDb {
    /// The settings for `engine` hosting `database` — the ONE place the starter
    /// page's credentials are decided, so the file on disk and the SQL rexenv
    /// runs can never disagree about which server they mean.
    pub fn for_engine(engine: crate::core::db::DbEngine, database: &str) -> Self {
        Self {
            engine: match engine {
                crate::core::db::DbEngine::Mariadb => "MariaDB",
                _ => "MySQL",
            },
            host: "127.0.0.1".into(),
            port: engine.port(),
            database: database.to_string(),
            username: "root".into(),
            password: String::new(),
        }
    }
}

const INDEX_TEMPLATE: &str = include_str!("../../templates/starter/index.php");
const DB_TEMPLATE: &str = include_str!("../../templates/starter/db.php");

/// Write the starter page into `docroot` — and `db.php` beside it when the site
/// has a starter database.
///
/// Existing files are LEFT ALONE, each independently: a user who deleted
/// `db.php` and kept their edited `index.php` gets neither back.
pub fn write_files(docroot: &Path, db: Option<&StarterDb>) -> Result<()> {
    let index = docroot.join("index.php");
    if !index.exists() {
        std::fs::write(index, INDEX_TEMPLATE)?;
    }
    if let Some(db) = db {
        let path = docroot.join("db.php");
        if !path.exists() {
            std::fs::write(path, render_db_php(db))?;
        }
    }
    Ok(())
}

/// `db.php` with this site's connection substituted in.
///
/// Placeholder replacement rather than `format!`: the template is real PHP with
/// real braces in it, and a format string would need every one of them doubled —
/// which is how a generated file stops being a file anyone can read.
fn render_db_php(db: &StarterDb) -> String {
    DB_TEMPLATE
        .replace("{{ENGINE}}", db.engine)
        .replace("{{HOST}}", &db.host)
        .replace("{{PORT}}", &db.port.to_string())
        .replace("{{DATABASE}}", &db.database)
        .replace("{{USERNAME}}", &db.username)
        .replace("{{PASSWORD}}", &db.password)
        .replace("{{TABLE}}", TABLE)
}

/// Create the starter table in `database` and seed it — idempotently, so a
/// retry (or a second create against a database that already exists) adds
/// nothing and destroys nothing.
///
/// `IF NOT EXISTS` covers the table; the seed's `WHERE NOT EXISTS` covers the
/// rows. The alternative — DROP and recreate — would silently delete whatever
/// the developer had put in the table by the time they hit Retry.
pub fn seed(client: &SqlClient, port: u16, database: &str) -> Result<()> {
    database::validate_db_name(database)?;
    database::mysql_exec(
        client,
        port,
        &format!("USE `{database}`; {}", seed_sql()),
        &format!("seeding the starter table in `{database}`"),
    )
}

/// The DDL + seed, as one script. Kept beside the page that reads it: the column
/// list here and the `SELECT` in `index.php` are one fact in two files.
fn seed_sql() -> String {
    format!(
        "CREATE TABLE IF NOT EXISTS `{TABLE}` (
            `id`         INT UNSIGNED NOT NULL AUTO_INCREMENT,
            `title`      VARCHAR(120) NOT NULL,
            `note`       VARCHAR(255) NOT NULL,
            `created_at` TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP,
            PRIMARY KEY (`id`)
        ) ENGINE=InnoDB DEFAULT CHARSET=utf8mb4 COLLATE=utf8mb4_unicode_ci;
        INSERT INTO `{TABLE}` (`title`, `note`)
        SELECT * FROM (
            SELECT 'Hello from your database' AS t,
                   'rexenv created it and seeded this row when the site was created.' AS n
            UNION ALL SELECT 'Edit this page',
                   'index.php is in your site folder. Change it, refresh, keep going.'
            UNION ALL SELECT 'The connection is one file',
                   'db.php returns a ready PDO. Host 127.0.0.1, user root, no password.'
            UNION ALL SELECT 'Browse the data',
                   'The Database tab in rexenv opens this database in Adminer.'
        ) AS seed
        WHERE NOT EXISTS (SELECT 1 FROM `{TABLE}`);"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_php_carries_this_site_and_no_placeholders() {
        let db = StarterDb::for_engine(crate::core::db::DbEngine::Mariadb, "php_shop_rex");
        let out = render_db_php(&db);
        assert!(out.contains("'engine'   => 'MariaDB',"));
        assert!(out.contains("'port'     => 13307,"), "{out}");
        assert!(out.contains("'database' => 'php_shop_rex',"));
        assert!(out.contains("'table'    => 'starter_items',"));
        // A placeholder that survives is a PHP parse error on the site's first
        // page load — the one failure this file cannot report from inside PHP.
        assert!(!out.contains("{{"), "unsubstituted placeholder in db.php:\n{out}");
    }

    #[test]
    fn the_page_reads_the_table_the_seed_creates() {
        // Two files, one fact. The SELECT in the page names columns the DDL
        // must define; a rename in either that misses the other renders an
        // error card on a brand-new site.
        for column in ["id", "title", "note", "created_at"] {
            assert!(seed_sql().contains(&format!("`{column}`")), "DDL is missing {column}");
            assert!(INDEX_TEMPLATE.contains(&format!("`{column}`")), "page is missing {column}");
        }
        // The page never hardcodes the table name — it reads it from db.php.
        assert!(!INDEX_TEMPLATE.contains(TABLE));
    }

    /// A throwaway docroot under the system temp dir — the fixture owns every
    /// path it removes (the 24 Jul rule: never `rm -rf` a derived parent).
    fn scratch_docroot(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir()
            .join(format!("rexenv-starter-{}-{name}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn existing_files_are_never_overwritten() {
        let root = &scratch_docroot("keep");
        std::fs::write(root.join("index.php"), "<?php // mine\n").unwrap();
        let db = StarterDb::for_engine(crate::core::db::DbEngine::Mysql, "php_a_rex");
        write_files(root, Some(&db)).unwrap();
        assert_eq!(std::fs::read_to_string(root.join("index.php")).unwrap(), "<?php // mine\n");
        // The one that WAS missing is still written — the two files are decided
        // independently, so a deleted db.php does not resurrect an edited page.
        assert!(std::fs::read_to_string(root.join("db.php")).unwrap().contains("php_a_rex"));
        std::fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn no_database_means_no_db_php() {
        let root = &scratch_docroot("nodb");
        write_files(root, None).unwrap();
        assert!(root.join("index.php").exists());
        assert!(!root.join("db.php").exists(), "a site with no database got a connection file");
        std::fs::remove_dir_all(root).unwrap();
    }
}
