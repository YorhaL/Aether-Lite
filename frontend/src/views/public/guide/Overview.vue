<script setup lang="ts">
import { ref } from 'vue'
import {
  Code,
  Server,
  Key,
  Container,
  Shield,
  Monitor,
  Check,
  Copy,
  Zap,
} from 'lucide-vue-next'
import { panelClasses } from './guide-config'

// 部署步骤数据
const activeDeployTab = ref(0)
const copiedStep = ref<string | null>(null)

const productionSteps = [
  {
    title: '克隆代码',
    code: 'git clone --branch master --single-branch https://github.com/YorhaL/Aether.git\ncd Aether',
    icon: Code
  },
  {
    title: '配置环境变量',
    note: '生成密钥并填入 .env',
    code: 'cp .env.example .env\n./generate_keys.sh',
    icon: Key
  },
  {
    title: '部署 / 更新',
    note: '自动执行数据库迁移',
    code: 'docker compose pull && docker compose up -d',
    icon: Container
  },
  {
    title: '升级前备份',
    note: '可选',
    code: 'docker compose exec postgres pg_dump -U postgres aether | gzip > backup_$(date +%Y%m%d_%H%M%S).sql.gz',
    icon: Shield,
    optional: true
  }
]

const developmentSteps = [
  {
    title: '启动依赖',
    note: '可选，make dev 会自动启动',
    code: 'docker compose up -d postgres redis',
    icon: Container
  },
  {
    title: '安装前端依赖',
    note: '首次本地开发',
    code: '(cd frontend && npm install)',
    icon: Server
  },
  {
    title: '启动开发服务',
    note: '后端 + Vite dev server',
    code: 'make dev',
    icon: Monitor
  }
]

function copyStep(stepId: string, code: string) {
  navigator.clipboard.writeText(code)
  copiedStep.value = stepId
  setTimeout(() => {
    copiedStep.value = null
  }, 2000)
}
</script>

<template>
  <div class="space-y-12">
    <!-- Hero 区域 -->
    <div class="space-y-4">
      <div class="inline-flex items-center gap-1.5 rounded-full bg-[#cc785c]/10 dark:bg-[#cc785c]/20 border border-[#cc785c]/20 dark:border-[#cc785c]/40 px-3 py-1 text-xs font-medium text-[#cc785c] dark:text-[#d4a27f]">
        <Zap class="h-3 w-3" />
        Aether Lite 官方文档
      </div>
      <h1 class="text-3xl font-bold text-[#262624] dark:text-[#f1ead8]">
        快速开始
      </h1>
      <p class="text-base text-[#666663] dark:text-[#a3a094] max-w-2xl">
        本文档将引导您完成 Aether Lite 的部署，并配置用于内部 API 分发的自定义提供商。
      </p>
    </div>

    <!-- 1. 项目部署 -->
    <section
      id="production"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>1. 项目部署</h2>
      
      <div
        :class="[panelClasses.card]"
        class="mt-6"
      >
        <!-- Tab 切换 -->
        <div class="flex border-b border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] px-5">
          <button
            v-for="(tab, idx) in [
              { icon: Container, label: 'Docker 预构建镜像' },
              { icon: Monitor, label: '本地开发' }
            ]"
            :key="idx"
            class="flex items-center gap-2 px-4 py-3 text-sm font-medium whitespace-nowrap transition-colors border-b-2 -mb-px hover:text-[#262624] dark:hover:text-[#f1ead8]"
            :class="activeDeployTab === idx
              ? 'border-[#cc785c] text-[#cc785c] dark:text-[#d4a27f]'
              : 'border-transparent text-[#666663] dark:text-[#a3a094]'"
            @click="activeDeployTab = idx"
          >
            <component
              :is="tab.icon"
              class="h-4 w-4"
            />
            {{ tab.label }}
          </button>
        </div>

        <!-- 生产环境步骤 -->
        <div
          v-show="activeDeployTab === 0"
          class="p-5 space-y-3"
        >
          <div
            v-for="(step, idx) in productionSteps"
            :key="idx"
            class="group rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] overflow-hidden transition-colors"
            :class="step.optional ? 'border-dashed opacity-80' : ''"
          >
            <div class="flex items-center gap-3 px-4 py-3">
              <span class="w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold flex-shrink-0 bg-[#cc785c] text-white">
                {{ idx + 1 }}
              </span>
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="text-sm font-medium text-[#262624] dark:text-[#f1ead8]">{{ step.title }}</span>
                  <span
                    v-if="step.optional"
                    class="text-[10px] px-1.5 py-0.5 rounded-full bg-[#e5e4df] dark:bg-[rgba(227,224,211,0.12)] text-[#666663] dark:text-[#a3a094]"
                  >
                    可选
                  </span>
                </div>
                <span
                  v-if="step.note"
                  class="text-xs text-[#91918d] dark:text-[#a3a094]/80"
                >{{ step.note }}</span>
              </div>
              <button
                class="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs text-[#666663] dark:text-[#a3a094] hover:bg-[#f0f0eb] dark:hover:bg-[#3a3731] transition-colors shrink-0"
                @click="copyStep(`prod-${idx}`, step.code)"
              >
                <Check
                  v-if="copiedStep === `prod-${idx}`"
                  class="h-3.5 w-3.5 text-green-500"
                />
                <Copy
                  v-else
                  class="h-3.5 w-3.5"
                />
                {{ copiedStep === `prod-${idx}` ? '已复制' : '复制' }}
              </button>
            </div>
            <pre class="px-4 pb-3 text-[13px] font-mono text-[#262624] dark:text-[#f1ead8] overflow-x-auto leading-relaxed border-t border-[#e5e4df]/50 dark:border-[rgba(227,224,211,0.06)] pt-3 mx-4 mb-1"><code>{{ step.code }}</code></pre>
          </div>
        </div>

        <!-- 开发环境步骤 -->
        <div
          v-show="activeDeployTab === 1"
          class="p-5 space-y-3"
        >
          <div
            v-for="(step, idx) in developmentSteps"
            :key="idx"
            class="group rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] overflow-hidden transition-colors"
          >
            <div class="flex items-center gap-3 px-4 py-3">
              <span class="w-6 h-6 rounded-full flex items-center justify-center text-xs font-bold flex-shrink-0 bg-[#cc785c] text-white">
                {{ idx + 1 }}
              </span>
              <div class="flex-1 min-w-0">
                <div class="flex items-center gap-2">
                  <span class="text-sm font-medium text-[#262624] dark:text-[#f1ead8]">{{ step.title }}</span>
                </div>
                <span
                  v-if="step.note"
                  class="text-xs text-[#91918d] dark:text-[#a3a094]/80"
                >{{ step.note }}</span>
              </div>
              <button
                class="flex items-center gap-1.5 px-2.5 py-1 rounded-md text-xs text-[#666663] dark:text-[#a3a094] hover:bg-[#f0f0eb] dark:hover:bg-[#3a3731] transition-colors shrink-0"
                @click="copyStep(`dev-${idx}`, step.code)"
              >
                <Check
                  v-if="copiedStep === `dev-${idx}`"
                  class="h-3.5 w-3.5 text-green-500"
                />
                <Copy
                  v-else
                  class="h-3.5 w-3.5"
                />
                {{ copiedStep === `dev-${idx}` ? '已复制' : '复制' }}
              </button>
            </div>
            <pre class="px-4 pb-3 text-[13px] font-mono text-[#262624] dark:text-[#f1ead8] overflow-x-auto leading-relaxed border-t border-[#e5e4df]/50 dark:border-[rgba(227,224,211,0.06)] pt-3 mx-4 mb-1"><code>{{ step.code }}</code></pre>
          </div>
        </div>
      </div>
    </section>

    <!-- 2. 配置流程 -->
    <section
      id="config-steps"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>2. 配置流程</h2>

      <div class="space-y-8 mt-6">
        <div>
          <h3>1. 创建统一模型</h3>
          <p>以 Opus 4.6 为例, 其他模型同样添加即可, 非必要建议只添加官方支持的模型ID。</p>
          <img
            loading="lazy"
            src="/guide/quickstart-create-model.webp"
            alt="创建统一模型"
            class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm mt-4 w-full"
          >
        </div>

        <div>
          <h3>2. 添加提供商</h3>
          <p>Lite 版本仅支持自定义提供商；请求格式必须与端点 API 格式一致。</p>
          <img
            loading="lazy"
            src="/guide/quickstart-add-provider.webp"
            alt="添加提供商"
            class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm mt-4 w-full"
          >
        </div>

        <div>
          <h3>3. 添加端点</h3>
          <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mt-4">
            <img
              loading="lazy"
              src="/guide/quickstart-add-endpoint-1.webp"
              alt="添加端点 1"
              class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm w-full"
            >
            <img
              loading="lazy"
              src="/guide/quickstart-add-endpoint-2.webp"
              alt="添加端点 2"
              class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm w-full"
            >
          </div>
        </div>

        <div>
          <h3>4. 添加密钥</h3>
          <img
            loading="lazy"
            src="/guide/quickstart-add-key.webp"
            alt="添加密钥"
            class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm mt-4 w-full"
          >
        </div>

        <div>
          <h3>5. 关联全局模型</h3>
          <div class="grid grid-cols-1 md:grid-cols-2 gap-4 mt-4">
            <img
              loading="lazy"
              src="/guide/quickstart-link-model-1.webp"
              alt="关联全局模型 1"
              class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm w-full"
            >
            <img
              loading="lazy"
              src="/guide/quickstart-link-model-2.webp"
              alt="关联全局模型 2"
              class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm w-full"
            >
          </div>
        </div>

        <div>
          <h3>6. 模型映射</h3>
          <img
            loading="lazy"
            src="/guide/quickstart-model-mapping.webp"
            alt="模型映射"
            class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm mt-4 w-full"
          >
        </div>
      </div>
    </section>

    <!-- 3. 异步任务 -->
    <section
      id="async-tasks"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>3. 异步任务</h2>
      <p>需要有提供商端点支持。</p>
      
      <ul class="list-decimal pl-5 mt-4 text-[#666663] dark:text-[#a3a094] space-y-1">
        <li><span class="font-medium text-[#262624] dark:text-[#f1ead8]">Veo</span></li>
        <li><span class="font-medium text-[#262624] dark:text-[#f1ead8]">Sora</span></li>
      </ul>
    </section>
  </div>
</template>
