# OpenAI WebSocket 模式

Aether Lite 支持两种与 OpenAI 上游保持同格式的 WebSocket 协议：

- OpenAI Realtime：客户端连接 `GET /v1/realtime?model=<model>`，路由到 `openai:realtime` Endpoint。
- OpenAI Responses WebSocket：客户端连接 `GET /v1/responses`，通过文本帧发送 `response.create`，路由到 `openai:responses` Endpoint。

原有 `POST /v1/responses` HTTP/SSE 接口不受影响。Lite 只代理同格式协议，不提供 Realtime 与 Responses、Chat Completions 或其他格式之间的转换。

## Provider 配置

### Realtime

为 Provider 新增 API 格式为 `openai:realtime` 的 Endpoint，并配置支持该格式的 Key 与模型映射。OpenAI 官方 API 根地址可配置为 `https://api.openai.com/v1`，默认 Endpoint 路径为 `/realtime`。

Realtime 的模型在连接 URL 中传入：

```text
wss://aether.example.com/v1/realtime?model=gpt-realtime
```

网关会先完成鉴权、模型映射、余额容量检查、RPM 和并发准入，再连接上游；只有上游握手成功后才向客户端返回 `101 Switching Protocols`。建立连接后，Realtime JSON、音频及控制帧保持不透明的双向转发。

### Responses WebSocket

Responses WebSocket 与 HTTP Responses 共用 `openai:responses` Endpoint。由于兼容 `/v1/responses` HTTP 的提供商未必支持 WebSocket Upgrade，必须在 Provider 编辑页显式开启“Responses WebSocket”。该开关按 Provider 生效，请只在其 Responses Endpoint 均支持标准 WebSocket 协议时开启。

连接后发送标准事件：

```json
{
  "type": "response.create",
  "model": "gpt-5.2",
  "input": "hello"
}
```

网关按轮次处理请求。每轮都会重新检查当前 API Key 状态、IP/模型权限、RPM、每日用量、余额和上游并发，并应用当前请求体规则与模型映射；首轮选定上游后，同一连接的后续轮次必须继续匹配相同 Provider、Endpoint、Key、握手凭据和传输配置。绑定发生变化时，客户端应建立新连接。

## Lite 协议边界

Responses WebSocket 当前只支持隐式默认 lane：

- 同一连接同时只能有一个活动的 `response.create`。
- 客户端应等待 `response.completed`、`response.incomplete`、`response.failed` 或 `error` 后再发送下一轮。
- 命名 `stream_id` 会返回明确错误，不会被静默合并到默认 lane。
- `previous_response_id` 只能引用同一已鉴权连接中已观察到的 `response.completed` 或 `response.incomplete` 响应，防止用外部响应 ID 跨连接读取上下文。

Lite 不实现上游 Responses WebSocket 的多 lane 并发，也不会在连接建立后跨 Provider/Key 故障转移。需要命名 lane、并行 response 或新的路由绑定时，应使用独立 WebSocket 连接。

## 计费和限制

- Responses WebSocket 按每个终止事件记录一条请求生命周期和权威 token usage，因此支持有限余额 API Key。
- Realtime 按整条连接累计上游返回的文本/音频 token usage。当前为避免长连接用量在关闭前无法安全结算，有限余额 API Key 不开放 Realtime；无限内部额度不受此限制。
- 单帧和单消息最大 16 MiB，连接最长 60 分钟；Responses 客户端须在连接后 60 秒内发送首个事件。
- WebSocket 连接有独立的本机准入上限，可通过 `AETHER_GATEWAY_MAX_WEBSOCKET_CONNECTIONS` 配置。Redis 多节点部署可另设 `AETHER_GATEWAY_DISTRIBUTED_WEBSOCKET_CONNECTION_LIMIT`；未设置时复用分布式请求上限，设为 `0` 表示只使用本机上限。

反向代理必须转发 HTTP/1.1 WebSocket Upgrade、`Connection` 和 `Upgrade` 头，并允许至少 60 分钟的空闲连接。客户端鉴权继续使用 Aether API Key；服务端会移除下游握手凭据和连接级头，再使用选定 Provider Key 发起独立的上游握手。

协议事件格式以 [OpenAI Responses WebSocket 文档](https://developers.openai.com/api/docs/guides/websocket-mode) 和 [OpenAI Realtime 文档](https://developers.openai.com/api/docs/guides/realtime-websocket) 为准。
