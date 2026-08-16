<p align="center">
  <img src="frontend/public/aether_adaptive.svg" width="120" height="120" alt="Aether Lite Logo">
</p>

<h1 align="center">Aether Lite</h1>

<p align="center">
  <strong>面向团队内部 API 分发的自托管 AI 网关</strong><br>
  提供自定义上游接入、用户与 API Key 管理、额度控制、流控和运行状态监控
</p>

<p align="center">
  <a href="https://github.com/YorhaL/Aether-Lite/actions/workflows/lite-docker.yml">
    <img src="https://github.com/YorhaL/Aether-Lite/actions/workflows/lite-docker.yml/badge.svg?branch=master" alt="Lite Docker Build">
  </a>
</p>

## 功能

- 自定义 Provider、Endpoint、凭据和模型路由。
- 用户、用户组、API Key 与权限管理。
- 客户端与目标 Endpoint 使用相同 API 格式时的请求和响应透传。
- 上游负载均衡、健康监控、用量记录和基础额度管理。
- 系统、用户组、用户和 API Key 级别的统一流控策略。
- Web 管理后台。
- PostgreSQL 和 SQLite 数据库。

Lite 定位和长期演进边界见 [Lite 版本策略](docs/architecture/lite-edition-strategy.md)，数据库约束见 [Lite 数据库演进策略](docs/architecture/lite-database-strategy.md)。

## 快速部署

要求 Docker Engine 和 Docker Compose Plugin。

```bash
git clone --branch master --single-branch https://github.com/YorhaL/Aether-Lite.git
cd Aether-Lite
./install.sh
```

安装器会选择 SQLite 单节点或 PostgreSQL + Redis，生成 `.env` 和随机密钥，设置管理员密码并启动容器。

也可以手动初始化：

```bash
cp .env.example .env
./generate_keys.sh
```

在 `.env` 中至少设置以下内容：

```dotenv
APP_IMAGE=ghcr.io/yorhal/aether-lite:latest
ADMIN_PASSWORD=replace-with-a-strong-password
DB_PASSWORD=replace-with-a-database-password
REDIS_PASSWORD=replace-with-a-redis-password
```

`latest` 对应最新稳定版。也可以固定为 `1.2.3` 等具体版本，或使用持续跟踪 `master` 分支的 `edge`。

### PostgreSQL + Redis

适合服务器和团队部署：

```bash
docker compose pull
docker compose up -d
```

### SQLite 单节点

适合个人、小团队或轻量部署，数据保存在部署目录的 `./data`：

```bash
docker compose -f docker-compose.single-node.yml pull
docker compose -f docker-compose.single-node.yml up -d
```

部署完成后访问 `http://服务器地址:8084`。查看日志：

```bash
docker compose logs -f app
```

SQLite 部署需要同时指定 Compose 文件：

```bash
docker compose -f docker-compose.single-node.yml logs -f app
```

### 更新

`APP_IMAGE` 使用 `latest`、`edge` 或其他可变标签时，可以直接拉取并重建应用容器：

```bash
./update.sh
```

SQLite 单节点部署使用：

```bash
./update.sh --mode single-node
```

固定具体版本时，先修改 `.env` 中的 `APP_IMAGE`，再执行更新命令。持久化数据库数据不会随应用容器重建而删除；生产环境更新前仍应先备份数据库。

## 发布镜像

推送 `vX.Y.Z` 标签会自动构建 `linux/amd64` 和 `linux/arm64` 镜像、创建对应的 GitHub Release，并发布到：

```text
ghcr.io/yorhal/aether-lite
```

创建稳定版本：

```bash
git switch master
git pull --ff-only origin master
git tag v1.0.0
git push origin v1.0.0
```

对应镜像标签：

```text
ghcr.io/yorhal/aether-lite:1.0.0
ghcr.io/yorhal/aether-lite:1.0
ghcr.io/yorhal/aether-lite:latest
```

预发布标签支持 `v1.0.0-beta.1` 和 `v1.0.0-rc.1`。每次推送 `master` 分支也会更新 `edge` 和对应的 `sha-*` 镜像。

## 本地开发

需要 Rust toolchain、Node.js、npm、Docker 和 Make：

```bash
cp .env.example .env
make dev
```

`make dev` 会启动 Rust 网关和前端开发服务器。也可以分别运行：

```bash
make dev-backend
make dev-frontend
```

常用检查：

```bash
cargo fmt --all --check
cargo test --workspace

cd frontend
npm ci
npm run lint
npm run test:run
```

## API 与数据

Provider 使用管理员配置的 API 格式。运行时按原请求和响应格式透传，因此客户端格式应与目标 Endpoint 的格式一致。

相关接口文档：

- [Provider 接口定义](docs/api/provider-interface-definitions.md)
- [Embeddings API](docs/api/embeddings.md)
- [Rerank API](docs/api/rerank.md)

数据库支持：

- SQLite：单节点运行，运行时协调使用进程内存。
- PostgreSQL：可配合 Redis 进行多实例运行时协调。

Lite 专有数据使用独立迁移链和命名空间，核心 PostgreSQL/SQLite 迁移保持兼容。备份和恢复要求见 [Lite 数据库演进策略](docs/architecture/lite-database-strategy.md)。

## 常用环境变量

| 变量 | 用途 |
| --- | --- |
| `APP_IMAGE` | Docker 镜像及版本，推荐固定到正式版本 |
| `APP_PORT` | HTTP 监听端口，默认 `8084` |
| `JWT_SECRET_KEY` | 登录令牌签名密钥 |
| `ENCRYPTION_KEY` | Provider 凭据等敏感数据的加密密钥 |
| `ADMIN_USERNAME` | 首次启动时创建的管理员用户名 |
| `ADMIN_PASSWORD` | 首次启动时创建的管理员密码 |
| `DB_NAME` / `DB_USER` / `DB_PASSWORD` | PostgreSQL Compose 数据库配置 |
| `REDIS_PASSWORD` | Redis Compose 认证密码 |
| `RUST_LOG` | Rust 日志过滤规则 |
| `CORS_ORIGINS` | 允许访问网关的跨域来源 |

完整配置及说明见 [.env.example](.env.example)。

## 许可证

本项目采用 [Aether 非商业开源许可证](LICENSE)。使用、修改和分发前请阅读许可证全文。
