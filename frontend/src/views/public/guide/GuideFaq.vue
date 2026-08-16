<script setup lang="ts">
import { reactive } from 'vue'
import { HelpCircle, ChevronRight } from 'lucide-vue-next'

const faqs = reactive([
  {
    id: 1,
    question: '1. 为什么发生错误没有进行故障转移？',
    answer: '故障转移机制（Failover）依赖于配置的重试策略和上游返回的错误码类型。通常只有在遇到 429 (Too Many Requests) 或 5xx (Server Errors) 时，并且在”最大重试次数”允许的范围内，才会触发故障转移。对于 400 (Bad Request) 或 401 (Unauthorized) 这种客户端确切错误，为避免持续无效重试，系统可能直接中断请求并返回错误。同时也要检查调度模式是否支持故障转移（例如固定顺序模式或缓存亲和性强绑定时可能会影响行为）。',
    isOpen: true
  },
  {
    id: 2,
    question: '2. 如何排查请求不通的问题？',
    answer: '建议首先检查【系统设置】中的请求体记录等级是否设为 `Full`，以便抓取完整的上下行数据。然后检查对应的【提供商端点】的 Base URL 和 API 格式是否正确匹配。如果开启了带来，排查【代理配置】优先级。',
    isOpen: false
  }
])

const toggleFaq = (index: number) => {
  faqs[index].isOpen = !faqs[index].isOpen
}
</script>

<template>
  <div class="space-y-12 pb-12">
    <!-- Hero 区域 -->
    <div class="space-y-4">
      <div class="inline-flex items-center gap-1.5 rounded-full bg-[#cc785c]/10 dark:bg-[#cc785c]/20 border border-[#cc785c]/20 dark:border-[#cc785c]/40 px-3 py-1 text-xs font-medium text-[#cc785c] dark:text-[#d4a27f]">
        <HelpCircle class="h-3 w-3" />
        答疑解惑
      </div>
      <h1 class="text-3xl font-bold text-[#262624] dark:text-[#f1ead8]">
        常见问题
      </h1>
      <p class="text-base text-[#666663] dark:text-[#a3a094] max-w-2xl">
        在使用 Aether Lite 过程中遇到的常见问题与排错指南。
      </p>
    </div>

    <section class="scroll-mt-24 lg:scroll-mt-20">
      <div class="space-y-4 mt-8">
        <div
          v-for="(faq, index) in faqs"
          :key="faq.id"
          class="bg-white/50 dark:bg-white/5 border border-[#e5e4df] dark:border-[rgba(227,224,211,0.06)] rounded-xl overflow-hidden shadow-sm transition-all"
        >
          <button
            class="w-full flex items-center justify-between p-5 text-left hover:bg-black/5 dark:hover:bg-white/5 transition-colors"
            @click="toggleFaq(index)"
          >
            <h3 class="text-lg font-medium text-[#262624] dark:text-[#f1ead8] m-0">
              {{ faq.question }}
            </h3>
            <ChevronRight
              class="w-5 h-5 text-[#91918d] dark:text-[#a3a094] transition-transform duration-200"
              :class="{ 'rotate-90 text-[#cc785c]': faq.isOpen }"
            />
          </button>
          
          <div
            v-show="faq.isOpen"
            class="px-5 pb-5 pt-0 text-sm text-[#666663] dark:text-[#a3a094] leading-relaxed border-t border-[#e5e4df]/50 dark:border-[rgba(227,224,211,0.06)] mt-2"
          >
            <div class="pt-4">
              {{ faq.answer }}
            </div>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>
