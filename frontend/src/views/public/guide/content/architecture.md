# 架构说明

Aether Lite 的系统架构、请求处理流程和数据流向。

## 1. 系统概览

```mermaid
graph LR
    Client[客户端<br>SDK / CLI / Web]
    AetherLite[Aether Lite<br>认证 / 路由 / 编排]
    PostgreSQL[(PostgreSQL)]
    Redis[(Redis)]
    Upstream[上游供应商<br>Claude / OpenAI / Gemini]

    Client -->|API Request| AetherLite
    AetherLite -.->|Auth / Config| PostgreSQL
    AetherLite <.-.>|Cache / Quota / Lock| Redis
    AetherLite -->|Proxy Request| Upstream
```

## 2. 核心原则

1. **API格式、端点、认证方式说明**
   Aether Lite 在接收请求后，会按原始 API 格式统一管理路由。

2. **统一的入口模型名称**
   在内部完成自定义提供商和模型映射管理。客户端只需知道统一的模型名，Aether Lite 会根据映射规则找到对应的上游模型名称。

3. **请求流转：同格式透传**
   `API 格式入口` → `同格式请求透传` → `上游提供商` → `同格式响应透传` → `API 格式出口`。
