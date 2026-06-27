//! core::services — service supervision (Phase 1: PHP-FPM pools).
//!
//! One PHP-FPM master per PHP version (not per site) — all sites on a version
//! share one pool, listening on a loopback TCP port for FastCGI. Nginx (6.2)
//! connects to this port. We run php-fpm as the current user (no root), so the
//! pool needs no `user`/`group` directive.

use crate::error::{Error, Result};
use crate::platform::traits::Platform;
use std::net::{Ipv4Addr, SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::process::Child;
use std::time::Duration;

/// Loopback FastCGI port for the PHP 8.3 pool (one pool per PHP version).
pub const PHP_FPM_PORT: u16 = 9783;

/// Render a php-fpm config: one foreground `[global]` master + one `[www]` pool
/// on `127.0.0.1:<port>`. The pool generator is per-version so more versions can
/// be added later, each on its own port.
pub fn generate_fpm_config(port: u16, pid_file: &Path, log_file: &Path) -> String {
    format!(
        "[global]\n\
         pid = {pid}\n\
         error_log = {log}\n\
         daemonize = no\n\
         \n\
         [www]\n\
         listen = 127.0.0.1:{port}\n\
         pm = dynamic\n\
         pm.max_children = 5\n\
         pm.start_servers = 2\n\
         pm.min_spare_servers = 1\n\
         pm.max_spare_servers = 3\n\
         catch_workers_output = yes\n",
        pid = pid_file.display(),
        log = log_file.display(),
    )
}

/// Write the php-fpm config for `version` (on `port`) under the config dir,
/// creating the run/log dirs. Returns the config path.
pub fn write_fpm_config(platform: &dyn Platform, version: &str, port: u16) -> Result<PathBuf> {
    let config_dir = platform.paths().config_dir()?;
    let log_dir = platform.paths().log_dir()?;
    let run_dir = platform.paths().app_data_dir()?.join("run");
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&log_dir)?;
    std::fs::create_dir_all(&run_dir)?;

    let conf = config_dir.join(format!("php-fpm-{version}.conf"));
    let pid = run_dir.join(format!("php-fpm-{version}.pid"));
    let log = log_dir.join(format!("php-fpm-{version}.log"));
    std::fs::write(&conf, generate_fpm_config(port, &pid, &log))?;
    Ok(conf)
}

/// Start the php-fpm master in the foreground (`-F`) with the given config, via
/// `ProcessSupervisor`. Returns the child handle (track its pid to `stop`).
pub fn start_fpm(platform: &dyn Platform, php_fpm_bin: &Path, conf: &Path) -> Result<Child> {
    let args = vec![
        "-F".to_string(),
        "-y".to_string(),
        conf.display().to_string(),
    ];
    platform.supervisor().spawn(php_fpm_bin, &args)
}

/// Validate a php-fpm config without starting it (`php-fpm -t -y <conf>`).
pub fn test_fpm_config(platform: &dyn Platform, php_fpm_bin: &Path, conf: &Path) -> Result<()> {
    let args = vec![
        "-t".to_string(),
        "-y".to_string(),
        conf.display().to_string(),
    ];
    let mut child = platform.supervisor().spawn(php_fpm_bin, &args)?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "php-fpm config test failed (exit {:?})",
            status.code()
        )))
    }
}

/// Stop a running php-fpm by pid.
pub fn stop(platform: &dyn Platform, pid: u32) -> Result<()> {
    platform.supervisor().stop(pid)
}

/// Service status the services/UI layer reads: a pool is `running` iff its
/// loopback FastCGI port accepts a connection.
pub fn fpm_running(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

// ── Shared Nginx ────────────────────────────────────────────────────────────
//
// One shared nginx process listens on an internal loopback HTTP port (plain
// HTTP — TLS is Caddy's job) and dispatches to a per-site `server` block by
// `server_name`. Each block roots at the site's docroot and proxies `.php` to
// the shared php-fpm pool. Caddy proxies all `*.test` to this port.

/// Internal loopback HTTP port the shared nginx listens on (Caddy's upstream).
pub const NGINX_HTTP_PORT: u16 = 8088;

/// WordPress rewrite mode for a site. Phase 1 exercises `Single`; the multisite
/// slots exist so Phase 3 needs no refactor.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RewriteMode {
    Single,
    SubdomainMultisite,
    SubdirectoryMultisite,
}

/// One site's nginx server block.
#[derive(Debug, Clone)]
pub struct NginxSite {
    pub domain: String,
    pub docroot: PathBuf,
    pub php_fpm_port: u16,
    pub rewrite: RewriteMode,
}

/// Full shared-nginx configuration.
#[derive(Debug, Clone)]
pub struct NginxConfig {
    pub http_port: u16,
    pub pid: PathBuf,
    pub error_log: PathBuf,
    pub access_log: PathBuf,
    /// Directory for nginx's writable temp paths (created on start).
    pub temp_root: PathBuf,
    pub sites: Vec<NginxSite>,
}

/// The `location /` (and any rewrite) block for a site's mode. Single uses plain
/// try_files; subdirectory multisite adds WordPress's network rewrite rules.
fn rewrite_block(mode: RewriteMode) -> String {
    match mode {
        // Subdomain multisite: each subdomain is its own WP host, so the routing
        // is the same as single — the difference is in wp-config + DNS/SSL.
        RewriteMode::Single | RewriteMode::SubdomainMultisite => {
            "\t\tlocation / {\n\
             \t\t\ttry_files $uri $uri/ /index.php?$args;\n\
             \t\t}\n"
                .to_string()
        }
        // Subdirectory multisite: WordPress's official network rewrite rules.
        RewriteMode::SubdirectoryMultisite => {
            "\t\tlocation / {\n\
             \t\t\ttry_files $uri $uri/ /index.php?$args;\n\
             \t\t}\n\
             \t\tif (!-e $request_filename) {\n\
             \t\t\trewrite /wp-admin$ $scheme://$host$uri/ permanent;\n\
             \t\t\trewrite ^(/[^/]+)?(/wp-.*) $2 last;\n\
             \t\t\trewrite ^(/[^/]+)?(/.*\\.php) $2 last;\n\
             \t\t}\n"
                .to_string()
        }
    }
}

/// FastCGI params passed to php-fpm (inlined so we don't depend on an external
/// `fastcgi_params` file). `HTTPS=on` is set when Caddy forwards an https request.
fn fcgi_params() -> &'static str {
    "\t\t\tfastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;\n\
     \t\t\tfastcgi_param QUERY_STRING $query_string;\n\
     \t\t\tfastcgi_param REQUEST_METHOD $request_method;\n\
     \t\t\tfastcgi_param CONTENT_TYPE $content_type;\n\
     \t\t\tfastcgi_param CONTENT_LENGTH $content_length;\n\
     \t\t\tfastcgi_param SCRIPT_NAME $fastcgi_script_name;\n\
     \t\t\tfastcgi_param REQUEST_URI $request_uri;\n\
     \t\t\tfastcgi_param DOCUMENT_URI $document_uri;\n\
     \t\t\tfastcgi_param DOCUMENT_ROOT $document_root;\n\
     \t\t\tfastcgi_param SERVER_PROTOCOL $server_protocol;\n\
     \t\t\tfastcgi_param GATEWAY_INTERFACE CGI/1.1;\n\
     \t\t\tfastcgi_param SERVER_SOFTWARE nginx;\n\
     \t\t\tfastcgi_param REMOTE_ADDR $remote_addr;\n\
     \t\t\tfastcgi_param REMOTE_PORT $remote_port;\n\
     \t\t\tfastcgi_param SERVER_ADDR $server_addr;\n\
     \t\t\tfastcgi_param SERVER_PORT $server_port;\n\
     \t\t\tfastcgi_param SERVER_NAME $server_name;\n\
     \t\t\tfastcgi_param REQUEST_SCHEME $scheme;\n\
     \t\t\tfastcgi_param HTTPS $rexenv_https if_not_empty;\n"
}

fn server_block(http_port: u16, site: &NginxSite) -> String {
    format!(
        "\n\tserver {{\n\
         \t\tlisten 127.0.0.1:{port};\n\
         \t\tserver_name {domain};\n\
         \t\troot \"{root}\";\n\
         \t\tindex index.php index.html;\n\
         {rewrite}\
         \t\tlocation ~ \\.php$ {{\n\
         \t\t\tfastcgi_pass 127.0.0.1:{fpm};\n\
         \t\t\tfastcgi_index index.php;\n\
         {params}\
         \t\t}}\n\
         \t}}\n",
        port = http_port,
        domain = site.domain,
        root = site.docroot.display(),
        fpm = site.php_fpm_port,
        rewrite = rewrite_block(site.rewrite),
        params = fcgi_params(),
    )
}

/// Render the shared nginx config: one `http {}` with a `server {}` per site,
/// each routed by `server_name` and proxying `.php` to php-fpm.
pub fn generate_nginx_config(cfg: &NginxConfig) -> String {
    let mut s = String::new();
    s.push_str("worker_processes 1;\n");
    s.push_str("daemon off;\n"); // foreground for ProcessSupervisor
    // Quote path values: app-data paths contain spaces ("Application Support").
    s.push_str(&format!("pid \"{}\";\n", cfg.pid.display()));
    s.push_str(&format!("error_log \"{}\";\n", cfg.error_log.display()));
    s.push_str("events {\n\tworker_connections 256;\n}\n\n");
    s.push_str("http {\n");
    s.push_str(&format!("\taccess_log \"{}\";\n", cfg.access_log.display()));
    s.push_str(
        "\ttypes {\n\
         \t\ttext/html html htm;\n\
         \t\ttext/css css;\n\
         \t\tapplication/javascript js;\n\
         \t\timage/png png;\n\
         \t\timage/jpeg jpg jpeg;\n\
         \t\timage/gif gif;\n\
         \t\timage/svg+xml svg;\n\
         \t\tapplication/json json;\n\
         \t}\n",
    );
    s.push_str("\tdefault_type application/octet-stream;\n");
    s.push_str("\tsendfile on;\n");
    s.push_str("\tclient_max_body_size 128m;\n");
    s.push_str(&format!(
        "\tclient_body_temp_path \"{t}/client_body\";\n\
         \tfastcgi_temp_path \"{t}/fastcgi\";\n\
         \tproxy_temp_path \"{t}/proxy\";\n\
         \tuwsgi_temp_path \"{t}/uwsgi\";\n\
         \tscgi_temp_path \"{t}/scgi\";\n",
        t = cfg.temp_root.display()
    ));
    // Map Caddy's X-Forwarded-Proto to an HTTPS flag for php (WordPress is_ssl()).
    s.push_str("\tmap $http_x_forwarded_proto $rexenv_https {\n\t\tdefault '';\n\t\thttps on;\n\t}\n");
    for site in &cfg.sites {
        s.push_str(&server_block(cfg.http_port, site));
    }
    s.push_str("}\n");
    s
}

/// Write the shared nginx config + create its prefix/log/temp dirs. Returns
/// `(config_path, prefix_dir)`.
pub fn write_nginx_config(
    platform: &dyn Platform,
    http_port: u16,
    sites: Vec<NginxSite>,
) -> Result<(PathBuf, PathBuf)> {
    let config_dir = platform.paths().config_dir()?;
    let log_dir = platform.paths().log_dir()?;
    let prefix = platform.paths().app_data_dir()?.join("nginx");
    let temp_root = prefix.join("tmp");
    std::fs::create_dir_all(&config_dir)?;
    std::fs::create_dir_all(&log_dir)?;
    std::fs::create_dir_all(&temp_root)?;

    let cfg = NginxConfig {
        http_port,
        pid: prefix.join("nginx.pid"),
        error_log: log_dir.join("nginx-error.log"),
        access_log: log_dir.join("nginx-access.log"),
        temp_root,
        sites,
    };
    let conf = config_dir.join("nginx.conf");
    std::fs::write(&conf, generate_nginx_config(&cfg))?;
    Ok((conf, prefix))
}

fn nginx_args(conf: &Path, prefix: &Path, flag: Option<&str>) -> Vec<String> {
    let mut args = Vec::new();
    if let Some(f) = flag {
        args.push(f.to_string());
    }
    // Prefix must end with a separator for nginx.
    args.push("-p".to_string());
    args.push(format!("{}/", prefix.display()));
    args.push("-c".to_string());
    args.push(conf.display().to_string());
    args
}

/// Validate the nginx config (`nginx -t`).
pub fn test_nginx_config(
    platform: &dyn Platform,
    nginx_bin: &Path,
    conf: &Path,
    prefix: &Path,
) -> Result<()> {
    let mut child = platform
        .supervisor()
        .spawn(nginx_bin, &nginx_args(conf, prefix, Some("-t")))?;
    let status = child.wait()?;
    if status.success() {
        Ok(())
    } else {
        Err(Error::Other(format!(
            "nginx -t failed (exit {:?})",
            status.code()
        )))
    }
}

/// Start the shared nginx (foreground via `daemon off`) via `ProcessSupervisor`.
pub fn start_nginx(
    platform: &dyn Platform,
    nginx_bin: &Path,
    conf: &Path,
    prefix: &Path,
) -> Result<Child> {
    platform
        .supervisor()
        .spawn(nginx_bin, &nginx_args(conf, prefix, None))
}

/// True if the shared nginx is accepting connections on its loopback port.
pub fn nginx_running(port: u16) -> bool {
    TcpStream::connect_timeout(
        &SocketAddr::from((Ipv4Addr::LOCALHOST, port)),
        Duration::from_millis(300),
    )
    .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fpm_config_has_global_and_pool() {
        let cfg = generate_fpm_config(
            9783,
            Path::new("/run/php-fpm-8.3.pid"),
            Path::new("/logs/php-fpm-8.3.log"),
        );
        assert!(cfg.contains("[global]"));
        assert!(cfg.contains("daemonize = no"));
        assert!(cfg.contains("pid = /run/php-fpm-8.3.pid"));
        assert!(cfg.contains("error_log = /logs/php-fpm-8.3.log"));
        assert!(cfg.contains("[www]"));
        assert!(cfg.contains("listen = 127.0.0.1:9783"));
        assert!(cfg.contains("pm = dynamic"));
        // No user/group: we run as the current user, not root.
        assert!(!cfg.contains("\nuser ="));
        assert!(!cfg.contains("\ngroup ="));
    }

    #[test]
    fn fpm_running_false_on_closed_port() {
        // An unlikely-to-be-open high port: status should read stopped.
        assert!(!fpm_running(8))
    }

    fn nginx_cfg(mode: RewriteMode) -> NginxConfig {
        NginxConfig {
            http_port: 8088,
            pid: PathBuf::from("/run/nginx.pid"),
            error_log: PathBuf::from("/logs/nginx-error.log"),
            access_log: PathBuf::from("/logs/nginx-access.log"),
            temp_root: PathBuf::from("/tmp/rexenv-nginx"),
            sites: vec![NginxSite {
                domain: "acme.test".into(),
                docroot: PathBuf::from("/Sites/acme/public"),
                php_fpm_port: 9783,
                rewrite: mode,
            }],
        }
    }

    #[test]
    fn nginx_config_has_shared_listen_and_fastcgi() {
        let cfg = generate_nginx_config(&nginx_cfg(RewriteMode::Single));
        assert!(cfg.contains("daemon off;"));
        assert!(cfg.contains("listen 127.0.0.1:8088;"));
        assert!(cfg.contains("server_name acme.test;"));
        assert!(cfg.contains("root \"/Sites/acme/public\";"));
        assert!(cfg.contains("fastcgi_pass 127.0.0.1:9783;"));
        assert!(cfg.contains("fastcgi_param SCRIPT_FILENAME $document_root$fastcgi_script_name;"));
        assert!(cfg.contains("try_files $uri $uri/ /index.php?$args;"));
        // HTTPS flag mapped from Caddy's X-Forwarded-Proto.
        assert!(cfg.contains("map $http_x_forwarded_proto $rexenv_https"));
    }

    #[test]
    fn rewrite_slots_differ_by_mode() {
        // Single has no WP network rewrites; subdirectory multisite does.
        assert!(!generate_nginx_config(&nginx_cfg(RewriteMode::Single)).contains("rewrite /wp-admin$"));
        assert!(generate_nginx_config(&nginx_cfg(RewriteMode::SubdirectoryMultisite))
            .contains("rewrite /wp-admin$"));
        // Subdomain multisite routes like single (try_files), no path stripping.
        let sub = generate_nginx_config(&nginx_cfg(RewriteMode::SubdomainMultisite));
        assert!(sub.contains("try_files $uri $uri/ /index.php?$args;"));
        assert!(!sub.contains("rewrite /wp-admin$"));
    }

    #[test]
    fn nginx_renders_each_site() {
        let mut cfg = nginx_cfg(RewriteMode::Single);
        cfg.sites.push(NginxSite {
            domain: "two.test".into(),
            docroot: PathBuf::from("/Sites/two"),
            php_fpm_port: 9783,
            rewrite: RewriteMode::Single,
        });
        let out = generate_nginx_config(&cfg);
        assert!(out.contains("server_name acme.test;"));
        assert!(out.contains("server_name two.test;"));
        assert_eq!(out.matches("fastcgi_pass").count(), 2);
    }
}
