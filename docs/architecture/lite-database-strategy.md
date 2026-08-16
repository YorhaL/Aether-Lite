# Aether Lite 数据库演进策略

本文约束 Aether Lite 与主版本并行演进时的数据库边界。目标是让 Lite 能持续接收主版本的核心数据库变更，同时允许 Lite 增加自己的持久化能力，而不让两条迁移历史互相冲突。产品定位和非数据库差异的处理规则见 [Aether Lite 定位与主版本差异处理策略](lite-edition-strategy.md)。

## 支持范围

- Lite 只支持 PostgreSQL 和 SQLite。
- MySQL/MariaDB 不在支持范围内，不保留驱动、迁移、schema、配置或数据兼容逻辑。
- Lite 只支持从当前 Rust 主版本直接切换；不支持从旧 Python/Alembic 版本或其部署配置直接升级，也不保留 Python 运行时、过渡模式、兼容层或 wrapper。
- PostgreSQL 与 SQLite 的核心 schema 和迁移历史保持与主版本兼容。核心迁移中即使包含 Lite 已删除功能所使用的表或字段，也只作为数据库兼容结构保留，业务代码不得重新依赖这些结构。
- 核心 SQL 中保留的 Python/Alembic 历史注释与兼容建表语句仅用于匹配当前 Rust 主版本已发布迁移的版本号和 checksum，不代表 Lite 支持从 Python 版本迁入；这些历史迁移不得改写。

## 两条迁移链

数据库变更分为两类，必须分别维护：

1. **核心迁移**：来自主版本，继续使用主版本的迁移目录和 `_sqlx_migrations` 记录。
2. **Lite 扩展迁移**：只服务于 Lite 独有功能，使用独立目录、独立迁移记录和独立命名空间。

核心迁移历史一旦发布就不可修改、重排或复用版本号。同步主版本时，应原样接收新的核心迁移；不得把 Lite 独有变更插入核心迁移，也不得通过修改历史迁移来解决冲突。

Lite 扩展迁移不得复用 `_sqlx_migrations`。迁移执行器应使用独立记录表，例如 `_aether_lite_migrations`。应用启动时必须先执行核心迁移，再执行 Lite 扩展迁移；任一阶段失败都应中止启动。

## Lite 扩展数据的建模边界

- 不直接 `ALTER`、重命名或删除主版本核心表及其字段、索引和约束。
- 新增 Lite 独有数据时，建立 `lite_*` 扩展表，并通过核心实体的稳定主键关联。
- 不为 Lite 独有表向核心表增加反向外键或其他会改变核心 schema 的约束。
- Lite 功能删除后，由 Lite 扩展迁移负责其数据生命周期，不回写或改造核心迁移历史。
- 如果一项需求必须修改核心表才能成立，应先将其视为主版本数据库变更，而不是 Lite 私有扩展；确认两边都会采用后，再进入核心迁移链。

这些边界保证主版本的新迁移可以继续合入 Lite，并把冲突限制在明确的扩展层。

## PostgreSQL

- Lite 扩展对象使用独立的 `aether_lite` schema。
- Lite 扩展迁移记录放在该命名空间内，例如 `aether_lite._aether_lite_migrations`。
- 核心对象继续使用主版本既有 schema 和 `_sqlx_migrations`，不得由 Lite 扩展迁移接管。
- 访问跨 schema 的核心数据时应显式限定 schema，避免依赖部署环境中的 `search_path`。

## SQLite

- Lite 独有功能优先使用 sidecar 数据库文件，使核心数据库及其 `_sqlx_migrations` 与主版本保持原样。
- 只有在 Lite 数据与核心数据必须处于同一 SQLite 事务时，才允许把 `lite_*` 表放入核心数据库文件。
- 同一数据库文件中的 Lite 迁移仍必须由独立迁移执行器和 `_aether_lite_migrations` 记录管理，不能写入 `_sqlx_migrations`。
- sidecar 文件的路径、权限和生命周期必须随部署显式配置，不能临时拼接或隐式创建在不可备份的位置。

## 发布、备份与恢复

- 每次发布都应分别验证核心迁移和 Lite 扩展迁移，并覆盖从上一发布版本升级的路径。
- 备份、导出、导入和灾难恢复必须同时处理核心数据与 Lite 扩展数据。PostgreSQL 需要包含 `aether_lite` schema；SQLite 当前需要完整保留包含 `lite_*` 表的数据库文件，未来若引入 sidecar 还必须同时备份该文件。
- 恢复时先恢复并迁移核心数据库，再恢复并迁移 Lite 扩展数据。
- 不支持从 MySQL/MariaDB 迁入，也不提供兼容工具或过渡迁移。

## 变更检查清单

新增数据库功能前至少确认：

- 该变更属于主版本核心能力，还是 Lite 独有能力？
- Lite 独有变更是否完全位于独立迁移链和扩展表中？
- 是否避免修改任何已发布的核心迁移？
- PostgreSQL 与 SQLite 是否都有明确实现和升级测试？
- 备份、导入和恢复是否覆盖新增的扩展数据？
- 是否没有引入任何 MySQL/MariaDB 代码、配置、schema 或兼容分支？
