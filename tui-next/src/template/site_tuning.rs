//! Typed site settings, input validation and scoped configuration readback.

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub enum Http2Support {
    #[default]
    Unknown,
    Unsupported,
    Legacy,
    Modern,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Http2Mode {
    #[default]
    Auto,
    On,
    Off,
    Inherit,
}

impl Http2Mode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "自动",
            Self::On => "开启",
            Self::Off => "关闭",
            Self::Inherit => "继承",
        }
    }

    fn cycle(self, forward: bool) -> Self {
        let choices = [Self::Auto, Self::On, Self::Off, Self::Inherit];
        let current = choices.iter().position(|v| *v == self).unwrap_or(0);
        choices[(current + if forward { 1 } else { 3 }) % 4]
    }
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum GzipMode {
    #[default]
    Inherit,
    On,
    Off,
}

impl GzipMode {
    pub fn label(self) -> &'static str {
        match self {
            Self::Inherit => "继承",
            Self::On => "开启",
            Self::Off => "关闭",
        }
    }

    fn cycle(self, forward: bool) -> Self {
        let choices = [Self::Inherit, Self::On, Self::Off];
        let current = choices.iter().position(|v| *v == self).unwrap_or(0);
        choices[(current + if forward { 1 } else { 2 }) % 3]
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TuningField {
    Http2,
    Gzip,
    BodySize,
    ConnectTimeout,
    SendTimeout,
    ReadTimeout,
}

impl TuningField {
    pub const ALL: [Self; 6] = [
        Self::Http2,
        Self::Gzip,
        Self::BodySize,
        Self::ConnectTimeout,
        Self::SendTimeout,
        Self::ReadTimeout,
    ];

    pub fn key(self) -> &'static str {
        match self {
            Self::Http2 => "http2",
            Self::Gzip => "gzip",
            Self::BodySize => "body_size",
            Self::ConnectTimeout => "connect_timeout",
            Self::SendTimeout => "send_timeout",
            Self::ReadTimeout => "read_timeout",
        }
    }
}

#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct SiteTuning {
    pub http2: Http2Mode,
    pub gzip: GzipMode,
    pub body_size: String,
    pub connect_timeout: String,
    pub send_timeout: String,
    pub read_timeout: String,
}

impl SiteTuning {
    pub fn text_mut(&mut self, field: TuningField) -> Option<&mut String> {
        match field {
            TuningField::BodySize => Some(&mut self.body_size),
            TuningField::ConnectTimeout => Some(&mut self.connect_timeout),
            TuningField::SendTimeout => Some(&mut self.send_timeout),
            TuningField::ReadTimeout => Some(&mut self.read_timeout),
            _ => None,
        }
    }

    pub fn cycle(&mut self, field: TuningField, forward: bool) -> bool {
        match field {
            TuningField::Http2 => self.http2 = self.http2.cycle(forward),
            TuningField::Gzip => self.gzip = self.gzip.cycle(forward),
            _ => return false,
        }
        true
    }

    pub fn errors(&self, proxy: bool) -> Vec<(&'static str, String)> {
        if !proxy {
            return Vec::new();
        }
        [
            ("body_size", normalize_size(&self.body_size)),
            ("connect_timeout", normalize_timeout(&self.connect_timeout)),
            ("send_timeout", normalize_timeout(&self.send_timeout)),
            ("read_timeout", normalize_timeout(&self.read_timeout)),
        ]
        .into_iter()
        .filter_map(|(key, result)| result.err().map(|error| (key, error)))
        .collect()
    }

    pub fn normalized(&self, proxy: bool) -> Result<Self, String> {
        Ok(Self {
            http2: self.http2,
            gzip: self.gzip,
            body_size: if proxy {
                normalize_size(&self.body_size)?
            } else {
                String::new()
            },
            connect_timeout: if proxy {
                normalize_timeout(&self.connect_timeout)?
            } else {
                String::new()
            },
            send_timeout: if proxy {
                normalize_timeout(&self.send_timeout)?
            } else {
                String::new()
            },
            read_timeout: if proxy {
                normalize_timeout(&self.read_timeout)?
            } else {
                String::new()
            },
        })
    }

    /// Render only the client-facing HTTP/2 settings. TLS is a product constraint.
    pub fn http2_directives(
        &self,
        tls: bool,
        support: Http2Support,
    ) -> Result<(&'static str, &'static str), String> {
        if !tls || self.http2 == Http2Mode::Inherit {
            return Ok(("", ""));
        }
        match (self.http2, support) {
            (Http2Mode::Off, Http2Support::Modern) => Ok(("", "http2 off;")),
            (Http2Mode::Off, _) => Ok(("", "")),
            (_, Http2Support::Modern) => Ok(("", "http2 on;")),
            (_, Http2Support::Legacy) => Ok((" http2", "")),
            (Http2Mode::Auto, _) => Ok(("", "")),
            _ => Err(
                "无法启用 HTTP/2：未检测到兼容的 Nginx HTTP/2 模块，请检查 nginx -V 或选择自动"
                    .into(),
            ),
        }
    }
}

/// Bare UI numbers use MiB; `b` supports lossless readback of nginx byte values.
pub fn normalize_size(input: &str) -> Result<String, String> {
    normalize_quantity(input, true)
}

pub fn normalize_timeout(input: &str) -> Result<String, String> {
    normalize_quantity(input, false)
}

fn normalize_quantity(input: &str, size: bool) -> Result<String, String> {
    let input = if size {
        input.trim().to_ascii_lowercase()
    } else {
        input.trim().to_string()
    };
    if input.is_empty() {
        return Ok(String::new());
    }
    let digits = input.bytes().take_while(u8::is_ascii_digit).count();
    let (number, unit) = input.split_at(digits);
    let error = if size {
        "请输入非负整数，可带 k/m/g 单位（数字默认 MiB，0 不限）"
    } else {
        "请输入大于 0 的整数，可带 ms/s/m/h 单位（数字默认秒）"
    };
    let value = number.parse::<u64>().map_err(|_| error.to_string())?;
    let (unit, scale): (&str, u64) = if size {
        match unit {
            "" | "m" => ("m", 1024 * 1024),
            "k" => ("k", 1024),
            "g" => ("g", 1024 * 1024 * 1024),
            "b" => ("b", 1),
            _ => return Err(error.into()),
        }
    } else {
        match unit {
            "" | "s" => ("s", 1000),
            "ms" => ("ms", 1),
            "m" => ("m", 60_000),
            "h" => ("h", 3_600_000),
            _ => return Err(error.into()),
        }
    };
    let max = if size {
        i64::MAX as u64
    } else {
        i32::MAX as u64
    };
    if (!size && value == 0) || value.checked_mul(scale).map_or(true, |v| v > max) {
        return Err(if size {
            "上传大小超出范围"
        } else {
            "超时必须大于 0，且不超过 2147483647 毫秒"
        }
        .into());
    }
    if size && value == 0 {
        Ok("0".into())
    } else {
        Ok(format!("{value}{unit}"))
    }
}

#[derive(Debug)]
pub(crate) struct Directive {
    pub scope: Vec<String>,
    pub server: usize,
    pub name: String,
    pub value: String,
}

/// Small lexer: comments, quoted strings, escapes and block scope are respected.
/// It deliberately does not expand include files or evaluate nginx variables.
pub(crate) fn directives(content: &str) -> Vec<Directive> {
    let mut result = Vec::new();
    let mut scope = Vec::new();
    let mut words: Vec<String> = Vec::new();
    let mut word = String::new();
    let mut quote = None;
    let mut escaped = false;
    let mut comment = false;
    let mut server = 0;
    for ch in content.chars() {
        if comment {
            if ch == '\n' {
                comment = false;
            }
            continue;
        }
        if escaped {
            word.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
            continue;
        }
        if let Some(q) = quote {
            if ch == q {
                quote = None;
            } else {
                word.push(ch);
            }
            continue;
        }
        if ch == '\'' || ch == '"' {
            quote = Some(ch);
            continue;
        }
        if ch == '#' {
            comment = true;
            continue;
        }
        if ch.is_whitespace() || matches!(ch, ';' | '{' | '}') {
            if !word.is_empty() {
                words.push(std::mem::take(&mut word));
            }
            match ch {
                '{' => {
                    if words.first().is_some_and(|v| v == "server") {
                        server += 1;
                    }
                    scope.push(words.join(" "));
                    words.clear();
                }
                '}' => {
                    scope.pop();
                    words.clear();
                }
                ';' => {
                    if let Some(name) = words.first() {
                        result.push(Directive {
                            scope: scope.clone(),
                            server,
                            name: name.clone(),
                            value: words[1..].join(" "),
                        });
                    }
                    words.clear();
                }
                _ => {}
            }
        } else {
            word.push(ch);
        }
    }
    result
}

pub fn without_slots(content: &str) -> String {
    let mut in_slot = false;
    content
        .lines()
        .filter(|line| {
            let line = line.trim();
            if line.starts_with("# nginx-tools:custom-") {
                if line.ends_with(":start") {
                    in_slot = true;
                }
                if line.ends_with(":end") {
                    in_slot = false;
                }
                false
            } else {
                !in_slot
            }
        })
        .collect::<Vec<_>>()
        .join("\n")
}

pub fn parse_tuning(content: &str) -> SiteTuning {
    let managed = without_slots(content);
    let mut metadata_entries = Vec::new();
    let scoped = managed
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if let Some(json) = trimmed.strip_prefix("# nginx-tools:tuning=") {
                metadata_entries.push(serde_json::from_str::<SiteTuning>(json).ok());
                format!("ngtool_tuning_metadata {};", metadata_entries.len() - 1)
            } else if trimmed.starts_with("# nginx-tools:managed type=")
                || trimmed.starts_with("# nginx-tools:tool-marker: type=")
            {
                "ngtool_managed_marker yes;".into()
            } else if let Some(features) = trimmed.strip_prefix("# nginx-tools:features=") {
                format!("ngtool_features {features};")
            } else {
                line.to_string()
            }
        })
        .collect::<Vec<_>>()
        .join("\n");
    let mut tuning = SiteTuning {
        http2: Http2Mode::Inherit,
        ..Default::default()
    };
    let values = directives(&scoped);
    let server = values
        .iter()
        .find(|d| d.name == "ngtool_managed_marker")
        .or_else(|| values.iter().find(|d| d.name == "proxy_pass"))
        .or_else(|| values.iter().find(|d| d.name == "ssl_certificate"))
        .or_else(|| values.iter().find(|d| d.name == "root"))
        .map(|d| d.server)
        .unwrap_or(1);
    let metadata = values
        .iter()
        .find(|d| d.server == server && d.name == "ngtool_tuning_metadata")
        .and_then(|d| d.value.parse::<usize>().ok())
        .and_then(|index| metadata_entries.get(index))
        .and_then(Option::as_ref);
    let mut actual_http2 = None;
    // Parent values first, root-location overrides second, regardless of source order.
    for root_location in [false, true] {
        for d in values.iter().filter(|d| d.server == server) {
            let correct_scope = d.scope.last().is_some_and(|s| {
                s == if root_location {
                    "location /"
                } else {
                    "server"
                }
            });
            if !correct_scope {
                continue;
            }
            match d.name.as_str() {
                "http2" if !root_location => {
                    actual_http2 = Some(if d.value == "on" {
                        Http2Mode::On
                    } else {
                        Http2Mode::Off
                    })
                }
                "listen" if !root_location && d.value.split_whitespace().any(|v| v == "http2") => {
                    actual_http2 = Some(Http2Mode::On)
                }
                "gzip" if !root_location => {
                    tuning.gzip = if d.value == "on" {
                        GzipMode::On
                    } else {
                        GzipMode::Off
                    }
                }
                "client_max_body_size" if !root_location => {
                    tuning.body_size =
                        if d.value.bytes().all(|b| b.is_ascii_digit()) && d.value != "0" {
                            format!("{}b", d.value)
                        } else {
                            d.value.clone()
                        };
                }
                "proxy_connect_timeout" => tuning.connect_timeout = d.value.clone(),
                "proxy_send_timeout" => tuning.send_timeout = d.value.clone(),
                "proxy_read_timeout" => tuning.read_timeout = d.value.clone(),
                _ => {}
            }
        }
    }
    tuning.http2 = actual_http2.unwrap_or(Http2Mode::Inherit);
    if let Some(metadata) = metadata {
        tuning.http2 = match (metadata.http2, actual_http2) {
            (Http2Mode::Auto, Some(Http2Mode::On) | None) => Http2Mode::Auto,
            (mode, None)
                if !values
                    .iter()
                    .any(|d| d.server == server && d.name == "ssl_certificate") =>
            {
                mode
            }
            (Http2Mode::Off, None) => Http2Mode::Off,
            _ => tuning.http2,
        };
        // Keep implicit streaming defaults implicit, so disabling the feature removes them.
        let streaming = values
            .iter()
            .filter(|d| d.server == server && d.name == "ngtool_features")
            .flat_map(|d| d.value.split(','))
            .any(|feature| matches!(feature, "streaming" | "websocket"));
        if metadata.read_timeout.is_empty()
            && streaming
            && matches!(tuning.read_timeout.as_str(), "86400" | "86400s")
        {
            tuning.read_timeout.clear();
        }
    }
    tuning
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantity_units_and_bounds() -> Result<(), String> {
        for (input, output) in [
            ("50", "50m"),
            ("1G", "1g"),
            ("512k", "512k"),
            ("1024b", "1024b"),
            ("0", "0"),
            ("", ""),
        ] {
            assert_eq!(normalize_size(input)?, output);
        }
        for (input, output) in [
            ("300", "300s"),
            ("500ms", "500ms"),
            ("5m", "5m"),
            ("1h", "1h"),
            ("", ""),
        ] {
            assert_eq!(normalize_timeout(input)?, output);
        }
        for input in [
            "-1",
            "1.5",
            "1; return 200",
            "10mb",
            "9999999999999999999999",
            "999999999999g",
        ] {
            assert!(normalize_size(input).is_err(), "{input}");
        }
        for input in [
            "0",
            "-1",
            "1.5",
            "60xs",
            "1M",
            "1H",
            "2147483648ms",
            "999999h",
        ] {
            assert!(normalize_timeout(input).is_err(), "{input}");
        }
        Ok(())
    }

    #[test]
    fn scoped_readback_ignores_comments_slots_and_other_locations() {
        let tuning = parse_tuning(
            r#"
server { listen 80; return 301 https://$host$request_uri; }
server {
    listen 443 ssl http2;
    ssl_certificate /tmp/fullchain.pem;
    gzip on;
    client_max_body_size 1024;
    # proxy_read_timeout 1s;
    # nginx-tools:custom-before-location:start
    gzip off;
    # nginx-tools:custom-before-location:end
    location /other { proxy_read_timeout 5s; }
    location / { proxy_pass http://127.0.0.1:80; proxy_read_timeout 500ms; }
    proxy_read_timeout 9s;
    add_header X-Example "{ # proxy_read_timeout 3s; }";
}
"#,
        );
        assert_eq!(tuning.http2, Http2Mode::On);
        assert_eq!(tuning.gzip, GzipMode::On);
        assert_eq!(tuning.body_size, "1024b");
        assert_eq!(tuning.read_timeout, "500ms");
    }

    #[test]
    fn raw_changes_override_stale_metadata() {
        let tuning = parse_tuning(
            r#"
server {
    # nginx-tools:tuning={"http2":"auto","body_size":"50m","read_timeout":"300s"}
    ssl_certificate /tmp/cert;
    http2 off;
    client_max_body_size 1g;
    location / { proxy_read_timeout 900s; }
}
"#,
        );
        assert_eq!(tuning.http2, Http2Mode::Off);
        assert_eq!(tuning.body_size, "1g");
        assert_eq!(tuning.read_timeout, "900s");
    }

    #[test]
    fn managed_server_wins_over_preceding_acme_root() {
        let tuning = parse_tuning(
            r#"
server {
    root /var/www/acme;
    # nginx-tools:tuning={"http2":"off"}
    location /.well-known/acme-challenge/ { try_files $uri =404; }
}
server {
    # nginx-tools:managed type=proxy
    # nginx-tools:tuning={"http2":"auto"}
    ssl_certificate /tmp/cert;
    http2 on;
    gzip on;
    client_max_body_size 50m;
    location / { proxy_pass http://127.0.0.1:80; proxy_read_timeout 300s; }
}
"#,
        );
        assert_eq!(tuning.http2, Http2Mode::Auto);
        assert_eq!(tuning.gzip, GzipMode::On);
        assert_eq!(tuning.body_size, "50m");
        assert_eq!(tuning.read_timeout, "300s");
    }

    #[test]
    fn http2_capability_and_tls_gate() -> Result<(), String> {
        let mut tuning = SiteTuning::default();
        assert_eq!(
            tuning.http2_directives(true, Http2Support::Modern)?,
            ("", "http2 on;")
        );
        assert_eq!(
            tuning.http2_directives(true, Http2Support::Legacy)?,
            (" http2", "")
        );
        assert_eq!(
            tuning.http2_directives(true, Http2Support::Unsupported)?,
            ("", "")
        );
        tuning.http2 = Http2Mode::On;
        assert!(tuning
            .http2_directives(true, Http2Support::Unsupported)
            .is_err());
        assert!(tuning
            .http2_directives(true, Http2Support::Unknown)
            .is_err());
        assert_eq!(
            tuning.http2_directives(false, Http2Support::Modern)?,
            ("", "")
        );
        tuning.http2 = Http2Mode::Off;
        assert_eq!(
            tuning.http2_directives(true, Http2Support::Modern)?,
            ("", "http2 off;")
        );
        Ok(())
    }
}
