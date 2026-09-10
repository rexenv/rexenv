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

/// The connection `db.php` is generated with — the local-dev credentials the
/// site's engine runs with (loopback, the engine's superuser, no password).
#[derive(Debug, Clone)]
pub struct StarterDb {
    /// Display name of the engine, for the page and the generated comment.
    pub engine: &'static str,
    /// PDO's DSN prefix: `mysql` or `pgsql`. In the generated file rather than
    /// derived from the display name, because the page shows one and PDO needs
    /// the other and they are not the same string ("PostgreSQL" vs `pgsql`).
    pub driver: &'static str,
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
    ///
    /// Driver, superuser and port move together for the same reason they do in
    /// `laravel::DbSettings::for_engine` (ledger #546): a `pgsql` DSN on 13306
    /// reaches MySQL, and `root` on PostgreSQL is "role does not exist" — each
    /// an error naming neither engine, on a brand-new site's first page load.
    pub fn for_engine(engine: crate::core::db::DbEngine, database: &str) -> Self {
        use crate::core::db::DbEngine;
        let (label, driver, username) = match engine {
            DbEngine::Postgres => ("PostgreSQL", "pgsql", "postgres"),
            DbEngine::Mariadb => ("MariaDB", "mysql", "root"),
            _ => ("MySQL", "mysql", "root"),
        };
        Self {
            engine: label,
            driver,
            host: "127.0.0.1".into(),
            port: engine.port(),
            database: database.to_string(),
            username: username.into(),
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
        .replace("{{DRIVER}}", db.driver)
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
pub fn seed(
    engine: crate::core::db::DbEngine,
    client: &SqlClient,
    port: u16,
    database: &str,
) -> Result<()> {
    database::validate_db_name(database)?;
    engine.exec_in_database(
        client,
        port,
        database,
        &seed_sql(engine),
        &format!("seeding the starter table in `{database}`"),
    )
}

/// The four rows the seed inserts, as (title, note). Shared by both dialects so
/// a developer's first page says the same thing whichever engine they picked —
/// except the connection line, which is the one sentence that IS engine-specific
/// and would be a lie if it were shared.
fn seed_rows(db: &StarterDb) -> [(&'static str, String); 4] {
    [
        (
            "Hello from your database",
            "rexenv created it and seeded this row when the site was created.".into(),
        ),
        (
            "Edit this page",
            "index.php is in your site folder. Change it, refresh, keep going.".into(),
        ),
        (
            "The connection is one file",
            format!(
                "db.php returns a ready PDO. Host 127.0.0.1, port {}, user {}, no password.",
                db.port, db.username
            ),
        ),
        (
            "Browse the data",
            "The Database tab in rexenv opens this database in Adminer.".into(),
        ),
    ]
}

/// The DDL + seed, as one script, in the engine's own dialect. Kept beside the
/// page that reads it: the column list here and the `SELECT` in `index.php` are
/// one fact in two files.
///
/// The two dialects differ in more than quoting, which is why this is a match
/// and not a string with a swapped quote character: MySQL has
/// `INT UNSIGNED AUTO_INCREMENT`, `ENGINE=` and `CHARSET=`, none of which
/// PostgreSQL has any equivalent for; PostgreSQL has `IDENTITY` and needs the
/// literal rows wrapped in `VALUES` with a named alias for the anti-insert to
/// parse. **Both stay idempotent the same way** — `IF NOT EXISTS` for the table,
/// `WHERE NOT EXISTS` for the rows — because the alternative, DROP and recreate,
/// silently deletes whatever the developer put in the table before they hit
/// Retry.
fn seed_sql(engine: crate::core::db::DbEngine) -> String {
    let db = StarterDb::for_engine(engine, "unused");
    let rows = seed_rows(&db);
    let esc = |s: &str| s.replace('\'', "''");
    match engine {
        crate::core::db::DbEngine::Postgres => {
            let values = rows
                .iter()
                .map(|(t, n)| format!("('{}', '{}')", esc(t), esc(n)))
                .collect::<Vec<_>>()
                .join(",\n                   ");
            format!(
                "CREATE TABLE IF NOT EXISTS \"{TABLE}\" (
            \"id\"         INTEGER GENERATED BY DEFAULT AS IDENTITY PRIMARY KEY,
            \"title\"      VARCHAR(120) NOT NULL,
            \"note\"       VARCHAR(255) NOT NULL,
            \"created_at\" TIMESTAMP NOT NULL DEFAULT CURRENT_TIMESTAMP
        );
        INSERT INTO \"{TABLE}\" (\"title\", \"note\")
        SELECT * FROM (VALUES
                   {values}
        ) AS seed(t, n)
        WHERE NOT EXISTS (SELECT 1 FROM \"{TABLE}\");"
            )
        }
        _ => {
            let values = rows
                .iter()
                .map(|(t, n)| format!("SELECT '{}' AS t, '{}' AS n", esc(t), esc(n)))
                .collect::<Vec<_>>()
                .join("\n            UNION ALL ");
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
            {values}
        ) AS seed
        WHERE NOT EXISTS (SELECT 1 FROM `{TABLE}`);"
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn db_php_carries_this_site_and_no_placeholders() {
        let db = StarterDb::for_engine(crate::core::db::DbEngine::Mariadb, "php_shop_rex");
        let out = render_db_php(&db);
        assert!(out.contains("'database' => 'php_shop_rex',"));
        assert!(out.contains("'table'    => 'starter_items',"));
        // A placeholder that survives is a PHP parse error on the site's first
        // page load — the one failure this file cannot report from inside PHP.
        assert!(!out.contains("{{"), "unsubstituted placeholder in db.php:\n{out}");
    }

    #[test]
    fn the_page_reads_the_table_the_seed_creates_in_every_dialect() {
        use crate::core::db::DbEngine;
        // Two files, one fact, now three engines. The SELECT in the page names
        // columns each DDL must define; a rename in any of them that misses the
        // others renders an error card on a brand-new site.
        for engine in [DbEngine::Mysql, DbEngine::Mariadb, DbEngine::Postgres] {
            let ddl = seed_sql(engine);
            for column in ["id", "title", "note", "created_at"] {
                assert!(ddl.contains(column), "{engine:?} DDL is missing {column}");
                assert!(INDEX_TEMPLATE.contains(column), "page is missing {column}");
            }
            // Idempotent in both dialects, and by the same two clauses: a retry
            // must add nothing, and must not DROP what the developer typed.
            assert!(ddl.contains("IF NOT EXISTS"), "{engine:?} table is not idempotent");
            assert!(ddl.contains("WHERE NOT EXISTS"), "{engine:?} rows are not idempotent");
            assert!(!ddl.to_uppercase().contains("DROP "), "{engine:?} seed drops something");
        }

        // The page's query is unquoted so it parses on both, and the page never
        // hardcodes the table name — it reads it from db.php.
        assert!(
            INDEX_TEMPLATE.contains("SELECT id, title, note, created_at FROM {$table} ORDER BY id"),
            "the page's query must stay identifier-quote-free — backticks are MySQL-only"
        );
        assert!(!INDEX_TEMPLATE.contains(TABLE));
    }

    #[test]
    fn each_dialect_speaks_only_its_own_syntax() {
        use crate::core::db::DbEngine;
        let my = seed_sql(DbEngine::Mysql);
        let pg = seed_sql(DbEngine::Postgres);

        // MySQL-only spellings must not reach PostgreSQL, where each is a
        // syntax error on the site's first provision rather than a slow bug.
        for mysqlism in ["AUTO_INCREMENT", "ENGINE=InnoDB", "CHARSET=utf8mb4", "`"] {
            assert!(my.contains(mysqlism), "the MySQL DDL lost {mysqlism}");
            assert!(!pg.contains(mysqlism), "the PostgreSQL DDL carries {mysqlism}");
        }
        // …and PostgreSQL's own identity column is not sent to MySQL.
        assert!(pg.contains("GENERATED BY DEFAULT AS IDENTITY"));
        assert!(!my.contains("IDENTITY"));

        // The seeded prose is shared EXCEPT the connection line, which names the
        // port and user and would be a lie if it were shared: PostgreSQL's row
        // must say 15432/postgres, MySQL's 13306/root.
        assert!(pg.contains("port 15432, user postgres"), "{pg}");
        assert!(my.contains("port 13306, user root"), "{my}");
    }

    #[test]
    fn the_generated_connection_names_the_driver_pdo_takes() {
        use crate::core::db::DbEngine;
        // 'engine' is for the reader, 'driver' is for PDO, and conflating them
        // is a DSN of `PostgreSQL:host=…` — which fails as "could not find
        // driver", the error that sent this whole feature chasing a runtime bug.
        for (engine, label, driver, port, user) in [
            (DbEngine::Mysql, "MySQL", "mysql", 13306, "root"),
            (DbEngine::Mariadb, "MariaDB", "mysql", 13307, "root"),
            (DbEngine::Postgres, "PostgreSQL", "pgsql", 15432, "postgres"),
        ] {
            let out = render_db_php(&StarterDb::for_engine(engine, "php_shop_rex"));
            assert!(out.contains(&format!("'engine'   => '{label}',")), "{out}");
            assert!(out.contains(&format!("'driver'   => '{driver}',")), "{out}");
            assert!(out.contains(&format!("'port'     => {port},")), "{out}");
            assert!(out.contains(&format!("'username' => '{user}',")), "{out}");
            assert!(!out.contains("{{"), "unsubstituted placeholder:\n{out}");
        }
        // The template must build BOTH DSNs, and `charset` — a MySQL-only DSN
        // parameter PostgreSQL's driver rejects — must stay on the MySQL branch.
        assert!(DB_TEMPLATE.contains("'pgsql:host=%s;port=%d;dbname=%s'"));
        assert!(DB_TEMPLATE.contains("'mysql:host=%s;port=%d;dbname=%s;charset=utf8mb4'"));
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
