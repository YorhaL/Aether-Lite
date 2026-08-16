<script setup lang="ts">
import { Settings } from 'lucide-vue-next'
</script>

<template>
  <div class="space-y-12 pb-12">
    <!-- Hero 区域 -->
    <div class="space-y-4">
      <div class="inline-flex items-center gap-1.5 rounded-full bg-[#cc785c]/10 dark:bg-[#cc785c]/20 border border-[#cc785c]/20 dark:border-[#cc785c]/40 px-3 py-1 text-xs font-medium text-[#cc785c] dark:text-[#d4a27f]">
        <Settings class="h-3 w-3" />
        高级使用
      </div>
      <h1 class="text-3xl font-bold text-[#262624] dark:text-[#f1ead8]">
        高级功能
      </h1>
      <p class="text-base text-[#666663] dark:text-[#a3a094] max-w-2xl">
        解锁 Aether Lite 的自定义请求修改以及进阶配置。
      </p>
    </div>

    <!-- 1. 请求上游固定非流/流式 -->
    <section
      id="stream-policy"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>1. 请求上游固定非流/流式</h2>
      <p class="text-sm text-[#666663] dark:text-[#a3a094] mb-4">
        部分提供商在流式或非流式表现上可能存在差异，此功能可强制在请求上游时使用指定的流式/非流式格式（并在前端按用户请求还原）。
      </p>
    </section>

    <!-- 3. 请求头/体编辑 -->
    <section
      id="header-body-edit"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>2. 请求头/体编辑</h2>
      <p class="text-sm text-[#666663] dark:text-[#a3a094] mb-4">
        可在端点设置中添加规则（Rules），使用条件匹配与动作（如 Delete, Set, Insert）来动态修改发往上游的 Headers 或 JSON Payload。
      </p>
    </section>

    <!-- 4. 模型映射 -->
    <section
      id="model-mapping"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>3. 模型映射</h2>
      <p class="text-sm text-[#666663] dark:text-[#a3a094] mb-4">
        解决提供商特定模型名称与平台标准名称不一致的问题。将请求中的统一模型名动态转换为上游真实需要的模型名。
      </p>
    </section>

    <!-- 5. 正则映射 -->
    <section
      id="regex-mapping"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>4. 正则映射</h2>
      <ul class="list-decimal pl-5 space-y-2 mt-4 text-[#666663] dark:text-[#a3a094] text-sm">
        <li><strong class="text-[#262624] dark:text-[#f1ead8] font-medium">模型权限：</strong> 使用正则表达式来匹配一类模型，给予批量授权。</li>
        <li><strong class="text-[#262624] dark:text-[#f1ead8] font-medium">自动获取上游模型：</strong> 在自动获取的模型列表中，应用正则过滤与清洗，从而快速导入符合规则的标准模型列表。</li>
      </ul>
    </section>

    <div class="grid grid-cols-1 md:grid-cols-2 gap-8 mt-12 pt-8 border-t border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)]">
      <!-- 6. 余额监控 -->
      <section
        id="balance-monitor"
        class="scroll-mt-24 lg:scroll-mt-20"
      >
        <h3 class="mt-0 text-xl text-[#262624] dark:text-[#f1ead8]">
          6. 余额监控
        </h3>
        <p class="text-sm text-[#666663] dark:text-[#a3a094] mt-2">
          针对各大提供商的官方接口或常见聚合平台，自动抓取并记录剩余额度，在余额低于阈值时触发报警或禁用策略。
        </p>
      </section>

      <!-- 7. 配置导入/出 -->
      <section
        id="config-export"
        class="scroll-mt-24 lg:scroll-mt-20"
      >
        <h3 class="mt-0 text-xl text-[#262624] dark:text-[#f1ead8]">
          7. 配置导入/出
        </h3>
        <p class="text-sm text-[#666663] dark:text-[#a3a094] mt-2">
          支持将统一模型配置、提供商端点及网关路由策略一键导出为 JSON，并在其他部署实例中迁移导入。
        </p>
      </section>

      <!-- 8. 锁定用户密钥 -->
      <section
        id="lock-key"
        class="scroll-mt-24 lg:scroll-mt-20"
      >
        <h3 class="mt-0 text-xl text-[#262624] dark:text-[#f1ead8]">
          8. 锁定用户密钥
        </h3>
        <p class="text-sm text-[#666663] dark:text-[#a3a094] mt-2">
          若监控发现恶意使用、异常调用或高频报错，管理员可以临时或永久锁定特定密钥，以阻断攻击源头。
        </p>
      </section>
    </div>
  </div>
</template>
