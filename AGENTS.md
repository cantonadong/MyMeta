# MyMeta project rules

- Read `开发文档.md` (architecture, decisions, milestones) and `开发日志.md` (status, pitfalls) before changing anything; update the log after each milestone.
- UI follows MyBI `docs/ui-guidelines.md`: reuse its tokens and patterns (see `web/src/styles.css`); compact buttons; lists paginate 10 rows; toasts last 3 seconds.
- All interface text goes through `t()` with Simplified Chinese as source text and English in `web/src/locales/en.json`. Do not translate user data, object or column names.
- Times are stored and shown in UTC and labelled UTC.
- Sync only writes technical tables (`object_tech`, `column_tech`, `sync_run`, `change_log`, `todo`); manual business content lives in separate tables and is never overwritten by sync.
- New UI is previewed with `?mock` sample data (`web/src/mock.ts`) before real business logic.
- After every change, build the current code, force-restart the MyMeta service on its configured port, and verify that the replacement process responds.
- Work directly on main; only commit or push when asked.
