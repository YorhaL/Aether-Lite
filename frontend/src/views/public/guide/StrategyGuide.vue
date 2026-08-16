<script setup lang="ts">
import { Target } from 'lucide-vue-next'
</script>

<template>
  <div class="space-y-12 pb-12">
    <!-- Hero 区域 -->
    <div class="space-y-4">
      <div class="inline-flex items-center gap-1.5 rounded-full bg-[#cc785c]/10 dark:bg-[#cc785c]/20 border border-[#cc785c]/20 dark:border-[#cc785c]/40 px-3 py-1 text-xs font-medium text-[#cc785c] dark:text-[#d4a27f]">
        <Target class="h-3 w-3" />
        调度与策略
      </div>
      <h1 class="text-3xl font-bold text-[#262624] dark:text-[#f1ead8]">
        关键策略
      </h1>
      <p class="text-base text-[#666663] dark:text-[#a3a094] max-w-2xl">
        了解 Aether Lite 内部的日志记录、智能调度模式以及服务限制策略。
      </p>
    </div>

    <section
      id="request-logging"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>1. 请求体记录</h2>
      <p class="text-sm text-[#666663] dark:text-[#a3a094] mb-4">
        在系统设置中，您可以修改请求体记录详情等级，以便于调试和审计。
      </p>
      
      <div class="overflow-hidden rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] bg-white dark:bg-[#191714] shadow-sm max-w-2xl">
        <table class="w-full text-sm text-left">
          <thead class="bg-[#f5f5f0] dark:bg-[rgba(227,224,211,0.05)] border-b border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)]">
            <tr>
              <th
                scope="col"
                class="px-6 py-3 font-medium text-[#262624] dark:text-[#f1ead8]"
              >
                日志等级
              </th>
              <th
                scope="col"
                class="px-6 py-3 font-medium text-[#262624] dark:text-[#f1ead8]"
              >
                记录内容
              </th>
            </tr>
          </thead>
          <tbody class="divide-y divide-[#e5e4df] dark:divide-[rgba(227,224,211,0.06)]">
            <tr class="hover:bg-black/5 dark:hover:bg-white/5 transition-colors">
              <td class="px-6 py-4 font-mono font-medium text-[#cc785c] dark:text-[#d4a27f]">
                Base
              </td>
              <td class="px-6 py-4 text-[#666663] dark:text-[#a3a094]">
                基本请求信息（IP, 模型, 耗时, Token 等）
              </td>
            </tr>
            <tr class="hover:bg-black/5 dark:hover:bg-white/5 transition-colors">
              <td class="px-6 py-4 font-mono font-medium text-[#cc785c] dark:text-[#d4a27f]">
                Headers
              </td>
              <td class="px-6 py-4 text-[#666663] dark:text-[#a3a094]">
                Base + 请求头 (Headers)
              </td>
            </tr>
            <tr class="hover:bg-black/5 dark:hover:bg-white/5 transition-colors border-b-0">
              <td class="px-6 py-4 font-mono font-medium text-[#cc785c] dark:text-[#d4a27f]">
                Full
              </td>
              <td class="px-6 py-4 text-[#666663] dark:text-[#a3a094]">
                Headers + 完整的请求体与响应体 (Payloads)
              </td>
            </tr>
          </tbody>
        </table>
      </div>
      
      <img
        loading="lazy"
        src="/guide/strategy-request-logging.webp"
        alt="请求体记录设置"
        class="rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.12)] shadow-sm mt-6 w-full max-w-3xl"
      >
    </section>

    <section
      id="scheduling"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>2. 调度模式</h2>
      <ul class="list-decimal pl-5 space-y-2 mt-4 text-[#666663] dark:text-[#a3a094] text-sm">
        <li><strong class="text-[#262624] dark:text-[#f1ead8] font-medium">提供商优先：</strong> 优先根据提供商设置的顺序进行调度。</li>
        <li><strong class="text-[#262624] dark:text-[#f1ead8] font-medium">Key优先：</strong> 无视提供商层级，直接在所有可用的 Key 之间根据优先级进行调度。</li>
        <li><strong class="text-[#262624] dark:text-[#f1ead8] font-medium">缓存亲和：</strong> 尽量将相同用户的请求路由到之前处理过该用户请求的提供商/节点，以最大化利用上游缓存。</li>
        <li><strong class="text-[#262624] dark:text-[#f1ead8] font-medium">负载均衡：</strong> 在相同优先级的节点之间均匀分配流量。</li>
        <li><strong class="text-[#262624] dark:text-[#f1ead8] font-medium">固定顺序：</strong> 取消随机性与动态调整，严格按照固定的顺序遍历尝试。</li>
        <li><strong class="text-[#262624] dark:text-[#f1ead8] font-medium">故障转移：</strong> 当请求失败时，根据策略自动切换到下一个可用的备用节点进行重试。</li>
      </ul>
    </section>

    <section
      id="rate-limit"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>3. 访问限制</h2>
      <p class="text-sm text-[#666663] dark:text-[#a3a094] mb-4">
        系统支持多种维度的访问频率限制（Rate Limit），有效防止恶意请求或滥用，保障服务稳定性。
      </p>
    </section>

    <section
      id="payload-cleanup"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>4. 请求体压缩清理</h2>
      <p class="text-sm text-[#666663] dark:text-[#a3a094] mb-4">
        为节省数据库空间与提高查询性能，系统提供自动请求体清理与压缩策略，将历史请求详情定期冷热分离并清理。
      </p>
    </section>

    <section
      id="cron-tasks"
      class="scroll-mt-24 lg:scroll-mt-20"
    >
      <h2>5. 定时任务</h2>
      <p class="text-sm text-[#666663] dark:text-[#a3a094] mb-4">
        平台内置多个定时任务，用于模型列表同步、缓存清理、余额监控及统计数据聚合等周期性操作。
      </p>
    </section>
  </div>
</template>
