//! 应用状态：按页面拆分子状态，AppState 聚合事件与按键处理。

mod app;
mod backup;
mod certs;
mod common;
mod dashboard;
mod logs;
mod service;
mod site_edit;
mod site_form;
mod sites;

pub use app::AppState;
pub use certs::{CertsAction, CertsFocus};
pub use common::{FocusArea, Notification};
pub use logs::LogsFocus;
pub use service::ServiceButton;
pub use site_edit::EditFocus;
pub use site_form::{FormField, SiteTypeChoice};
pub use sites::SitesSortField;

#[cfg(test)]
mod tests {
    // 测试中使用 Default::default() 然后逐字段赋值更直观，关闭对应 lint。
    #![allow(clippy::field_reassign_with_default)]

    use super::*;
    use crate::app::state::logs::LogsState;
    use crate::app::state::site_edit::{char_to_byte, SiteEditState};
    use crate::app::state::sites::SitesState;
    use crate::domain::site::{Site, SiteType, SslStatus};
    use crate::template::config_parser::InjectionSlot;
    use std::path::PathBuf;

    fn test_site(name: &str, enabled: bool, site_type: SiteType, ssl: SslStatus) -> Site {
        Site {
            name: name.to_string(),
            primary_domain: None,
            all_domains: Vec::new(),
            access_log_path: None,
            error_log_path: None,
            site_type,
            target: None,
            enabled,
            ssl,
            config_path: PathBuf::from(format!("/tmp/{name}.conf")),
        }
    }

    #[test]
    fn char_to_byte_ascii() {
        assert_eq!(char_to_byte("hello", 0), 0);
        assert_eq!(char_to_byte("hello", 3), 3);
        assert_eq!(char_to_byte("hello", 5), 5);
        assert_eq!(char_to_byte("hello", 999), 5);
    }

    #[test]
    fn char_to_byte_cjk() {
        // 中文每字 3 字节
        let s = "应用代理";
        assert_eq!(char_to_byte(s, 0), 0);
        assert_eq!(char_to_byte(s, 1), 3);
        assert_eq!(char_to_byte(s, 2), 6);
        assert_eq!(char_to_byte(s, 4), 12);
        // 越界回到字节末尾
        assert_eq!(char_to_byte(s, 99), 12);
    }

    #[test]
    fn sites_sort_preserves_selected_site() {
        let mut sites = SitesState {
            list: vec![
                test_site("beta", false, SiteType::Static, SslStatus::None),
                test_site("alpha", true, SiteType::Proxy, SslStatus::None),
                test_site(
                    "gamma",
                    true,
                    SiteType::Emby,
                    SslStatus::Active { days_left: 9 },
                ),
            ],
            selected: 1,
            ..Default::default()
        };

        sites.sort_by = SitesSortField::Status;
        sites.sort_preserving_selection(Some("alpha"));

        assert_eq!(sites.list[0].name, "alpha");
        assert_eq!(sites.list[1].name, "gamma");
        assert_eq!(sites.list[2].name, "beta");
        assert_eq!(
            sites.current().map(|site| site.name.as_str()),
            Some("alpha")
        );
    }

    #[test]
    fn sites_cycle_sort_field_keeps_current_site() {
        let mut sites = SitesState {
            list: vec![
                test_site("b", false, SiteType::Unknown, SslStatus::None),
                test_site(
                    "a",
                    true,
                    SiteType::Proxy,
                    SslStatus::Active { days_left: 3 },
                ),
            ],
            selected: 0,
            ..Default::default()
        };

        sites.cycle_sort_field();

        assert_eq!(sites.sort_by, SitesSortField::Type);
        assert_eq!(sites.current().map(|site| site.name.as_str()), Some("b"));
    }

    #[test]
    fn replace_with_snippet_overrides_slot() {
        let mut s = SiteEditState::default();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "old".into());
        s.current_slot = InjectionSlot::BeforeLocation;
        s.replace_with_snippet("new value");
        assert_eq!(
            s.injection_slots.get(&InjectionSlot::BeforeLocation),
            Some(&"new value".to_string())
        );
        assert!(s.dirty);
    }

    #[test]
    fn append_snippet_concatenates_with_newline() {
        let mut s = SiteEditState::default();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "first;".into());
        s.current_slot = InjectionSlot::BeforeLocation;
        s.append_snippet("second;");
        assert_eq!(
            s.injection_slots.get(&InjectionSlot::BeforeLocation),
            Some(&"first;\nsecond;".to_string())
        );
    }

    #[test]
    fn seal_and_restore_original_recovers_form_fields() {
        let mut s = SiteEditState::default();
        s.domain = "app.example.com".into();
        s.domain_aliases = "www.app.example.com".into();
        s.target = "127.0.0.1:8080".into();
        s.upstream_scheme = "http".into();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "add_header X-K v;".into());
        s.seal_original();

        // 用户修改字段
        s.domain = "messy.example.com".into();
        s.domain_aliases = "www.messy.example.com".into();
        s.target = "0.0.0.0:80".into();
        s.upstream_scheme = "https".into();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "modified".into());
        s.dirty = true;

        // Ctrl+D 恢复
        assert!(s.restore_original());
        assert_eq!(s.domain, "app.example.com");
        assert_eq!(s.domain_aliases, "www.app.example.com");
        assert_eq!(s.target, "127.0.0.1:8080");
        assert_eq!(s.upstream_scheme, "http");
        assert_eq!(
            s.injection_slots.get(&InjectionSlot::BeforeLocation),
            Some(&"add_header X-K v;".to_string())
        );
        assert!(!s.dirty);
    }

    #[test]
    fn restore_original_returns_false_when_not_sealed() {
        let mut s = SiteEditState::default();
        assert!(!s.restore_original());
    }

    #[test]
    fn mark_saved_updates_raw_lines_and_original_snapshot() {
        let mut s = SiteEditState::default();
        s.domain = "old.example.com".into();
        s.domain_aliases = "www.old.example.com".into();
        s.target = "127.0.0.1:8080".into();
        s.raw_lines = vec!["old".into()];
        s.raw_cursor_line = 0;
        s.raw_cursor_col = 99;
        s.dirty = true;

        s.domain = "new.example.com".into();
        s.domain_aliases.clear();
        s.target = "127.0.0.1:9000".into();
        let saved_at = Some(std::time::SystemTime::now());
        s.mark_saved("line-1\nline-2\n", saved_at);

        assert_eq!(s.raw_lines, vec!["line-1", "line-2", ""]);
        assert_eq!(s.raw_cursor_line, 0);
        assert_eq!(s.raw_cursor_col, "line-1".chars().count());
        assert_eq!(s.mtime_at_load, saved_at);
        assert!(!s.dirty);

        s.domain = "mutated.example.com".into();
        assert!(s.restore_original());
        assert_eq!(s.domain, "new.example.com");
        assert_eq!(s.target, "127.0.0.1:9000");
    }

    #[test]
    fn site_edit_preserves_aliases_when_rendering() {
        let parsed = crate::template::config_parser::parse_for_edit(
            r#"
server {
    listen 80;
    server_name app.example.com www.app.example.com m.app.example.com;
    location / {
        proxy_pass http://127.0.0.1:8080;
    }
}
"#,
        );
        let s = SiteEditState::from_parsed("app", &parsed);
        assert_eq!(s.domain, "app.example.com");
        assert_eq!(s.domain_aliases, "www.app.example.com m.app.example.com");

        let params = s.build_render_params();
        assert_eq!(params.domain_name, "app.example.com");
        assert_eq!(
            params.domain_aliases,
            "www.app.example.com m.app.example.com"
        );
    }

    #[test]
    fn site_edit_empty_aliases_stays_empty() {
        // 模拟用户创建站点时没有填写附加域名的配置
        let parsed = crate::template::config_parser::parse_for_edit(
            r#"
server {
    listen 80;
    server_name app.example.com;
    location / {
        proxy_pass http://127.0.0.1:8080;
    }
}
"#,
        );

        // 验证解析结果
        assert_eq!(parsed.domains.len(), 1);
        assert_eq!(parsed.domains[0], "app.example.com");

        // 从解析结果创建编辑状态
        let s = SiteEditState::from_parsed("app", &parsed);
        assert_eq!(s.domain, "app.example.com");
        assert_eq!(s.domain_aliases, "", "附加域名应该为空字符串");

        // 验证渲染参数
        let params = s.build_render_params();
        assert_eq!(params.domain_name, "app.example.com");
        assert_eq!(params.domain_aliases, "", "渲染参数中的附加域名也应该为空");
    }

    #[test]
    fn site_edit_preserves_ssl_template_params() {
        let parsed = crate::template::config_parser::parse_for_edit(
            r#"
server {
    listen 443 ssl;
    server_name app.example.com;
    # nginx-tools:managed type=proxy
    ssl_certificate /etc/letsencrypt/live/app/fullchain.pem;
    ssl_certificate_key /etc/letsencrypt/live/app/privkey.pem;

    # nginx-tools:custom-before-location:start
    # nginx-tools:custom-before-location:end
    location / {
        proxy_pass http://127.0.0.1:8080;
        # nginx-tools:custom-inside-location:start
        # nginx-tools:custom-inside-location:end
    }
    # nginx-tools:custom-after-location:start
    # nginx-tools:custom-after-location:end
}
"#,
        );
        let state = SiteEditState::from_parsed("app", &parsed);
        let params = state.build_render_params();
        assert!(params.ssl_enabled);
        assert_eq!(
            params.ssl_cert_path,
            "/etc/letsencrypt/live/app/fullchain.pem"
        );
        assert_eq!(params.ssl_key_path, "/etc/letsencrypt/live/app/privkey.pem");
    }

    #[test]
    fn site_edit_preserves_selected_https_scheme_for_plain_target() {
        let parsed = crate::template::config_parser::parse_for_edit(
            r#"
server {
    listen 80;
    server_name app.example.com;
    location / {
        proxy_pass https://127.0.0.1:8443;
    }
}
"#,
        );

        let mut s = SiteEditState::from_parsed("app", &parsed);
        assert_eq!(s.upstream_scheme, "https");
        assert_eq!(s.target, "127.0.0.1:8443");

        let params = s.build_render_params();
        assert_eq!(params.upstream_scheme, "https");
        assert_eq!(params.upstream_target, "127.0.0.1:8443");

        s.upstream_scheme = "https".into();
        s.target = "127.0.0.1:8080".into();
        let params = s.build_render_params();
        assert_eq!(params.upstream_scheme, "https");
        assert_eq!(params.upstream_target, "127.0.0.1:8080");
    }

    #[test]
    fn raw_undo_redo_round_trip() {
        let mut s = SiteEditState::default();
        s.raw_lines = vec!["abc".into()];
        s.raw_cursor_line = 0;
        s.raw_cursor_col = 3;

        // 模拟一次写操作：先 push undo，再修改
        s.push_raw_undo();
        s.raw_lines[0].push('d');
        s.raw_cursor_col = 4;

        // undo 回到 "abc"
        assert!(s.raw_undo());
        assert_eq!(s.raw_lines, vec!["abc".to_string()]);
        assert_eq!(s.raw_cursor_col, 3);

        // redo 回到 "abcd"
        assert!(s.raw_redo());
        assert_eq!(s.raw_lines, vec!["abcd".to_string()]);
        assert_eq!(s.raw_cursor_col, 4);

        // 下次写操作清空 redo
        s.push_raw_undo();
        s.raw_lines[0].push('e');
        assert!(s.raw_redo_stack.is_empty());
    }

    #[test]
    fn raw_undo_empty_stack_returns_false() {
        let mut s = SiteEditState::default();
        assert!(!s.raw_undo());
        assert!(!s.raw_redo());
    }

    #[test]
    fn enter_slot_full_seeds_lines_from_injection_map() {
        let mut s = SiteEditState::default();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "line1;\nline2;".into());
        s.current_slot = InjectionSlot::BeforeLocation;
        s.enter_slot_full();

        assert_eq!(s.slot_edit_target, Some(InjectionSlot::BeforeLocation));
        assert_eq!(
            s.slot_edit_lines,
            vec!["line1;".to_string(), "line2;".to_string()]
        );
        assert_eq!(s.slot_edit_cursor_line, 0);
        assert_eq!(s.slot_edit_cursor_col, 0);
    }

    #[test]
    fn enter_slot_full_with_empty_slot_seeds_one_blank_line() {
        let mut s = SiteEditState::default();
        s.current_slot = InjectionSlot::InsideLocation;
        s.enter_slot_full();
        assert_eq!(s.slot_edit_lines, vec![String::new()]);
    }

    #[test]
    fn commit_slot_full_writes_back_and_marks_dirty() {
        let mut s = SiteEditState::default();
        s.current_slot = InjectionSlot::AfterLocation;
        s.enter_slot_full();
        s.slot_edit_lines = vec!["location /api { return 200; }".into()];

        let committed = s.commit_slot_full();
        assert_eq!(committed, Some(InjectionSlot::AfterLocation));
        assert_eq!(
            s.injection_slots.get(&InjectionSlot::AfterLocation),
            Some(&"location /api { return 200; }".to_string())
        );
        assert!(s.dirty);
    }

    #[test]
    fn commit_slot_full_empty_removes_slot() {
        let mut s = SiteEditState::default();
        s.injection_slots
            .insert(InjectionSlot::BeforeLocation, "old".into());
        s.current_slot = InjectionSlot::BeforeLocation;
        s.enter_slot_full();
        s.slot_edit_lines = vec![String::new()];
        s.commit_slot_full();
        assert!(!s
            .injection_slots
            .contains_key(&InjectionSlot::BeforeLocation));
    }

    #[test]
    fn slot_undo_redo_round_trip() {
        let mut s = SiteEditState::default();
        s.slot_edit_lines = vec!["abc".into()];
        s.push_slot_undo();
        s.slot_edit_lines[0].push('d');

        assert!(s.slot_undo());
        assert_eq!(s.slot_edit_lines, vec!["abc".to_string()]);
        assert!(s.slot_redo());
        assert_eq!(s.slot_edit_lines, vec!["abcd".to_string()]);
    }

    #[test]
    fn logs_scroll_vertical_clamps_and_updates_pause() {
        let mut logs = LogsState::default();
        logs.buffer = (0..20).map(|i| format!("line-{i}")).collect();

        logs.follow_tail(5);
        assert_eq!(logs.vertical_scroll, 15);
        assert!(!logs.paused);

        logs.scroll_vertical(-3, 5);
        assert_eq!(logs.vertical_scroll, 12);
        assert!(logs.paused);

        logs.scroll_vertical(999, 5);
        assert_eq!(logs.vertical_scroll, 15);
        assert!(!logs.paused);
    }

    #[test]
    fn logs_reveal_current_match_moves_into_view() {
        let mut logs = LogsState::default();
        logs.buffer = (0..30).map(|i| format!("line-{i}")).collect();
        logs.search("line-18");
        logs.vertical_scroll = 0;

        logs.reveal_current_match(5);

        assert_eq!(logs.vertical_scroll, 14);
        assert!(logs.paused);
    }

    #[test]
    fn logs_push_line_shifts_scroll_and_matches_when_buffer_rolls() {
        let mut logs = LogsState {
            max_lines: 3,
            vertical_scroll: 1,
            ..Default::default()
        };
        logs.buffer.push_back("a".into());
        logs.buffer.push_back("match-b".into());
        logs.buffer.push_back("match-c".into());
        logs.search("match");

        logs.push_line("d".into());

        assert_eq!(logs.buffer.len(), 3);
        assert_eq!(logs.vertical_scroll, 0);
        assert_eq!(logs.match_lines, vec![0, 1]);
    }

    #[test]
    fn logs_push_line_rebuilds_search_matches() {
        let mut logs = LogsState::default();
        logs.buffer.push_back("first error".into());
        logs.search("error");

        logs.push_line("second error".into());

        assert_eq!(logs.match_lines, vec![0, 1]);
        assert_eq!(logs.match_index, Some(0));
    }

    #[test]
    fn logs_push_line_removes_stale_matches_when_buffer_rolls() {
        let mut logs = LogsState {
            max_lines: 3,
            ..Default::default()
        };
        logs.buffer.push_back("old error".into());
        logs.buffer.push_back("plain".into());
        logs.buffer.push_back("new error".into());
        logs.search("error");
        logs.next_match();

        logs.push_line("plain tail".into());

        assert_eq!(
            logs.buffer.iter().cloned().collect::<Vec<_>>(),
            vec![
                "plain".to_string(),
                "new error".to_string(),
                "plain tail".to_string(),
            ]
        );
        assert_eq!(logs.match_lines, vec![1]);
        assert_eq!(logs.match_index, Some(0));
    }

    #[test]
    fn logs_follow_tail_keeps_last_lines_visible_after_append() {
        let mut logs = LogsState::default();
        let visible_lines = 5;

        let was_following = logs.is_following_tail(visible_lines);
        logs.push_line("line-1".into());
        if was_following && !logs.paused {
            logs.follow_tail(visible_lines);
        }

        assert_eq!(logs.vertical_scroll, 0);
        assert!(logs.buffer.front().is_some_and(|line| line == "line-1"));

        logs.buffer = (0..20).map(|i| format!("line-{i}")).collect();
        logs.follow_tail(visible_lines);
        let was_following = logs.is_following_tail(visible_lines);
        logs.push_line("line-20".into());
        if was_following && !logs.paused {
            logs.follow_tail(visible_lines);
        }

        assert_eq!(logs.vertical_scroll, 16);
        assert_eq!(logs.max_vertical_scroll(visible_lines), 16);
    }
}

