# 快速开始
## 部署

选择适合你的部署方式开始

### 1. 预构建镜像 (Docker Compose)
```markdown
# 1. 克隆代码
git clone https://github.com/fawney19/Aether.git
cd Aether

# 2. 配置环境变量
cp .env.example .env
./generate_keys.sh # 生成密钥, 并将生成的密钥填入 .env

# 3. 部署 / 更新（自动执行数据库迁移）
docker compose pull && docker compose up -d

# 4. 升级前备份
docker compose exec postgres pg_dump -U postgres aether | gzip > backup_$(date +%Y%m%d_%H%M%S).sql.gz
```

### 2. 本地开发
依赖 Docker、Rust toolchain、Node.js、make
```markdown
# 启动数据库（可选，make dev 会自动启动本地依赖）
docker compose up -d postgres redis

# 安装前端依赖（首次）
(cd frontend && npm install)

# 启动前后端
make dev
```

## 配置流程

1. **创建统一模型**
   以Opus4.6为例, 其他模型同样添加即可, 非必要建议只添加官方支持的模型ID
   ![image.png](/Aether%E4%BD%BF%E7%94%A8%E6%95%99%E7%A8%8B/image.png)

2. **添加提供商**
   ![image.png](/Aether%E4%BD%BF%E7%94%A8%E6%95%99%E7%A8%8B/image%201.png)

3. **添加端点**
   ![image.png](/Aether%E4%BD%BF%E7%94%A8%E6%95%99%E7%A8%8B/image%202.png)
   ![image.png](/Aether%E4%BD%BF%E7%94%A8%E6%95%99%E7%A8%8B/image%203.png)

4. **添加密钥**
   ![image.png](/Aether%E4%BD%BF%E7%94%A8%E6%95%99%E7%A8%8B/image%204.png)

5. **关联全局模型**
   ![image.png](/Aether%E4%BD%BF%E7%94%A8%E6%95%99%E7%A8%8B/image%205.png)
   ![image.png](/Aether%E4%BD%BF%E7%94%A8%E6%95%99%E7%A8%8B/image%206.png)

6. **模型映射**
   ![image.png](/Aether%E4%BD%BF%E7%94%A8%E6%95%99%E7%A8%8B/image%207.png)

## 异步任务

需要有提供商端点支持

1. Veo
2. Sora
