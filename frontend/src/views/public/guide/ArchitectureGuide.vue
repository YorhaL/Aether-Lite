<script setup lang="ts">
import { Network, ArrowRight } from 'lucide-vue-next'
import { panelClasses } from './guide-config'
import ArchitectureDiagram from './components/ArchitectureDiagram.vue'
</script>

<template>
  <div class="space-y-12">
    <!-- Hero 区域 -->
    <div class="space-y-4">
      <div class="inline-flex items-center gap-1.5 rounded-full bg-[#cc785c]/10 dark:bg-[#cc785c]/20 border border-[#cc785c]/20 dark:border-[#cc785c]/40 px-3 py-1 text-xs font-medium text-[#cc785c] dark:text-[#d4a27f]">
        <Network class="h-3 w-3" />
        系统架构
      </div>
      <h1 class="text-3xl font-bold text-[#262624] dark:text-[#f1ead8]">
        架构说明
      </h1>
    </div>

    <!-- 动态架构图 -->
    <div class="mt-10 mb-6">
      <ArchitectureDiagram />
    </div>

    <section>
      <div
        :class="[panelClasses.card]"
        class="p-6 mt-6"
      >
        <div class="space-y-5">
          <!-- 1. 同格式接入 -->
          <div class="space-y-1.5">
            <div class="flex items-center gap-2.5">
              <span class="text-xs font-medium font-mono text-[#cc785c]">1.</span>
              <span class="text-sm font-medium text-[#262624] dark:text-[#f1ead8]">同格式接入</span>
            </div>
            <p class="text-sm text-[#666663] dark:text-[#a3a094] leading-relaxed pl-[22px]">
              每个请求只会路由到 API 格式相同的端点，并以原始格式透传。
            </p>
          </div>

          <!-- 2. 统一模型路由 -->
          <div class="space-y-1.5">
            <div class="flex items-center gap-2.5">
              <span class="text-xs font-medium font-mono text-[#cc785c]">2.</span>
              <span class="text-sm font-medium text-[#262624] dark:text-[#f1ead8]">统一模型路由</span>
            </div>
            <p class="text-sm text-[#666663] dark:text-[#a3a094] leading-relaxed pl-[22px]">
              对外暴露统一模型名称，对内自动映射到各提供商的内部模型名及变体，支持优先级选择、亲和性路由、负载均衡与故障转移。
            </p>
          </div>

          <!-- 3. 同格式原生透传 -->
          <div class="space-y-2">
            <div class="flex items-center gap-2.5">
              <span class="text-xs font-medium font-mono text-[#cc785c]">3.</span>
              <span class="text-sm font-medium text-[#262624] dark:text-[#f1ead8]">同格式原生透传</span>
            </div>
            <div class="flex flex-col md:flex-row items-stretch md:items-center gap-2 bg-[#f5f5f0]/50 dark:bg-[rgba(227,224,211,0.05)] p-3 rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.06)]">
              <div class="flex-1 bg-white dark:bg-[#191714] border border-[#e5e4df] dark:border-white/10 rounded-lg py-2 px-3 text-center text-xs font-medium text-[#262624] dark:text-white/80">
                原生 API 入口
              </div>
              <ArrowRight class="hidden md:block w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 shrink-0" />
              <div class="md:hidden flex justify-center">
                <ArrowRight class="w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 rotate-90" />
              </div>
              <div class="flex-1 bg-[#cc785c]/5 dark:bg-[#cc785c]/10 border border-[#cc785c]/15 dark:border-[#cc785c]/20 rounded-lg py-2 px-3 text-center text-xs font-medium text-[#cc785c] dark:text-[#d4a27f]">
                同格式请求
              </div>
              <ArrowRight class="hidden md:block w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 shrink-0" />
              <div class="md:hidden flex justify-center">
                <ArrowRight class="w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 rotate-90" />
              </div>
              <div class="flex-1 bg-white dark:bg-[#191714] border border-[#e5e4df] dark:border-white/10 rounded-lg py-2 px-3 text-center text-xs font-medium text-[#262624] dark:text-white/80">
                上游提供商
              </div>
              <ArrowRight class="hidden md:block w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 shrink-0" />
              <div class="md:hidden flex justify-center">
                <ArrowRight class="w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 rotate-90" />
              </div>
              <div class="flex-1 bg-[#cc785c]/5 dark:bg-[#cc785c]/10 border border-[#cc785c]/15 dark:border-[#cc785c]/20 rounded-lg py-2 px-3 text-center text-xs font-medium text-[#cc785c] dark:text-[#d4a27f]">
                同格式响应
              </div>
              <ArrowRight class="hidden md:block w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 shrink-0" />
              <div class="md:hidden flex justify-center">
                <ArrowRight class="w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 rotate-90" />
              </div>
              <div class="flex-1 bg-white dark:bg-[#191714] border border-[#e5e4df] dark:border-white/10 rounded-lg py-2 px-3 text-center text-xs font-medium text-[#262624] dark:text-white/80">
                原生 API 响应
              </div>
            </div>
          </div>

          <!-- 4. 同生态原生透传 -->
          <div class="space-y-2">
            <div class="flex items-center gap-2.5">
              <span class="text-xs font-medium font-mono text-[#cc785c]">4.</span>
              <span class="text-sm font-medium text-[#262624] dark:text-[#f1ead8]">同生态原生透传</span>
            </div>
            <div class="flex flex-col md:flex-row items-stretch md:items-center gap-2 bg-[#f5f5f0]/50 dark:bg-[rgba(227,224,211,0.05)] p-3 rounded-xl border border-[#e5e4df] dark:border-[rgba(227,224,211,0.06)]">
              <div class="flex-1 bg-white dark:bg-[#191714] border border-[#e5e4df] dark:border-white/10 rounded-lg py-2 px-3 text-center text-xs font-medium text-[#262624] dark:text-white/80">
                原生 API 入口
              </div>
              <ArrowRight class="hidden md:block w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 shrink-0" />
              <div class="md:hidden flex justify-center">
                <ArrowRight class="w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 rotate-90" />
              </div>
              <div class="flex-[2] bg-[#cc785c]/5 dark:bg-[#cc785c]/10 border border-[#cc785c]/15 dark:border-[#cc785c]/20 rounded-lg py-2 px-3 text-center text-xs font-medium text-[#cc785c] dark:text-[#d4a27f]">
                原生请求透传 → 上游提供商 → 原生响应透传
              </div>
              <ArrowRight class="hidden md:block w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 shrink-0" />
              <div class="md:hidden flex justify-center">
                <ArrowRight class="w-4 h-4 text-[#cc785c]/40 dark:text-[#cc785c]/30 rotate-90" />
              </div>
              <div class="flex-1 bg-white dark:bg-[#191714] border border-[#e5e4df] dark:border-white/10 rounded-lg py-2 px-3 text-center text-xs font-medium text-[#262624] dark:text-white/80">
                原生 API 出口
              </div>
            </div>
          </div>
        </div>
      </div>
    </section>
  </div>
</template>
