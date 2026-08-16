<script setup lang="ts">
import { ref } from 'vue'
import { ArrowLeft } from 'lucide-vue-next'

const activeDemo = ref<number>(1)

const demos = [
  { id: 1, name: '垂直管道流 (Vertical Pipeline)' },
  { id: 2, name: '中心辐射 (Central Star Hub)' },
  { id: 3, name: '2.5D 层级结构 (Isometric Layered)' },
]
</script>

<template>
  <div class="fixed inset-0 z-50 bg-[#faf9f5] dark:bg-[#141311] overflow-y-auto">
    <div class="sticky top-0 z-10 flex items-center justify-between px-6 py-4 bg-white/80 dark:bg-[#1a1815]/80 backdrop-blur border-b border-[#e5e4df] dark:border-white/10">
      <div class="flex items-center gap-4">
        <router-link
          to="/guide/architecture"
          class="flex items-center gap-2 text-sm font-medium text-[#666663] hover:text-[#cc785c] dark:text-[#a3a094] transition-colors"
        >
          <ArrowLeft class="w-4 h-4" /> 返回文档
        </router-link>
        <h1 class="text-xl font-bold text-[#262624] dark:text-[#f1ead8] m-0 border-l border-[#e5e4df] dark:border-white/10 pl-4">
          架构布局草案演示
        </h1>
      </div>
      <div class="flex gap-2 bg-[#f5f5f0] dark:bg-[#1e1c19] p-1 rounded-xl border border-[#e5e4df] dark:border-white/5">
        <button
          v-for="demo in demos"
          :key="demo.id"
          class="px-4 py-2 text-sm font-medium rounded-lg transition-all duration-200"
          :class="[
            activeDemo === demo.id 
              ? 'bg-white dark:bg-[#2a2825] text-[#cc785c] shadow-sm' 
              : 'text-[#666663] dark:text-[#a3a094] hover:bg-black/5 dark:hover:bg-white/5'
          ]"
          @click="activeDemo = demo.id"
        >
          {{ demo.name }}
        </button>
      </div>
    </div>

    <!-- Canvas Area -->
    <div class="p-8 w-full max-w-7xl mx-auto flex justify-center min-h-[800px]">
      <!-- Variant 1: Vertical -->
      <div
        v-if="activeDemo === 1"
        class="w-full h-[800px] border border-dashed border-[#cc785c]/30 rounded-3xl relative flex flex-col items-center justify-between p-12"
      >
        <div class="absolute top-4 left-6 text-sm font-mono text-[#cc785c]/50">
          Layout Variant: Vertical Pipeline
        </div>
        <!-- Ingress row -->
        <div class="flex gap-12 w-full justify-center">
          <div class="w-48 h-16 bg-white dark:bg-[#1e1c19] border border-[#e5e4df] dark:border-white/10 rounded-xl flex items-center justify-center font-mono font-bold">
            Claude API Request
          </div>
          <div class="w-48 h-16 bg-white dark:bg-[#1e1c19] border border-[#e5e4df] dark:border-white/10 rounded-xl flex items-center justify-center font-mono font-bold">
            OpenAI API Request
          </div>
          <div class="w-48 h-16 bg-white dark:bg-[#1e1c19] border border-[#e5e4df] dark:border-white/10 rounded-xl flex items-center justify-center font-mono font-bold">
            Gemini API Request
          </div>
        </div>
        
        <!-- Down Arrows -->
        <div class="flex gap-12 w-full justify-center my-4 opacity-50">
          <div class="w-1 h-12 bg-[#cc785c] mx-auto relative">
            <div class="absolute bottom-0 left-1/2 -translate-x-1/2 translate-y-1/2 border-l-4 border-r-4 border-t-8 border-transparent border-t-[#cc785c]" />
          </div>
          <div class="w-1 h-12 bg-[#cc785c] mx-auto relative">
            <div class="absolute bottom-0 left-1/2 -translate-x-1/2 translate-y-1/2 border-l-4 border-r-4 border-t-8 border-transparent border-t-[#cc785c]" />
          </div>
          <div class="w-1 h-12 bg-[#cc785c] mx-auto relative">
            <div class="absolute bottom-0 left-1/2 -translate-x-1/2 translate-y-1/2 border-l-4 border-r-4 border-t-8 border-transparent border-t-[#cc785c]" />
          </div>
        </div>

        <!-- Aether Core -->
        <div class="w-full max-w-3xl h-48 bg-[#cc785c]/10 border-2 border-[#cc785c] rounded-2xl flex flex-col items-center justify-center">
          <h2 class="text-3xl font-black tracking-widest text-[#cc785c] mb-2">
            AETHER GATEWAY
          </h2>
          <div class="text-sm font-medium w-full px-12 text-center text-[#262624] dark:text-[#f1ead8]">
            <span class="px-3">多源聚合</span>|<span class="px-3">鉴权 / 配额管控</span>|<span class="px-3">负载均衡 / 故障转移</span>|<span class="px-3">基于亲和性路由</span>
          </div>
        </div>

        <!-- Split flow down -->
        <div class="w-full max-w-2xl h-16 relative flex justify-between px-24 my-4 opacity-50">
          <!-- Left line to convert -->
          <div class="absolute top-0 left-[25%] w-[1px] h-full bg-[#a855f7]" />
          <!-- Right line to passthrough -->
          <div class="absolute top-0 right-[25%] w-[1px] h-full bg-[#3b82f6]" />
        </div>

        <!-- Egress Processing -->
        <div class="flex gap-24 w-full justify-center">
          <div class="w-72 h-32 bg-[#a855f7]/10 border-2 border-[#a855f7] rounded-xl flex flex-col items-center justify-center text-[#a855f7]">
            <h3 class="font-bold text-lg mb-2">
              3. 同格式透传
            </h3>
            <span class="text-xs font-mono">Native Passthrough</span>
          </div>
          <div class="w-72 h-32 bg-[#3b82f6]/10 border-2 border-[#3b82f6] rounded-xl flex flex-col items-center justify-center text-[#3b82f6]">
            <h3 class="font-bold text-lg mb-2">
              4. 原生双向透传
            </h3>
            <span class="text-xs font-mono">Direct Passthrough</span>
          </div>
        </div>

        <!-- Final hop down -->
        <div class="w-1 h-12 bg-[#e5e4df] dark:bg-white/20 mx-auto my-4" />

        <!-- Upstream -->
        <div class="w-full max-w-4xl h-24 border-2 border-dashed border-[#e5e4df] dark:border-white/20 rounded-xl flex items-center justify-around">
          <div class="font-bold text-[#666663] dark:text-[#a3a094]">
            Upstream:
          </div>
          <div class="px-6 py-2 bg-emerald-500/10 text-emerald-600 rounded-lg font-bold">
            Claude
          </div>
          <div class="px-6 py-2 bg-emerald-500/10 text-emerald-600 rounded-lg font-bold">
            OpenAI
          </div>
          <div class="px-6 py-2 bg-emerald-500/10 text-emerald-600 rounded-lg font-bold">
            Gemini
          </div>
          <div class="px-6 py-2 bg-amber-500/10 text-amber-600 rounded-lg font-bold">
            Custom upstream APIs
          </div>
        </div>
      </div>

      <!-- Variant 2: Central Hub -->
      <div
        v-if="activeDemo === 2"
        class="w-full h-[800px] border border-dashed border-[#cc785c]/30 rounded-3xl relative flex items-center justify-center p-12"
      >
        <div class="absolute top-4 left-6 text-sm font-mono text-[#cc785c]/50">
          Layout Variant: Central Star/Hub
        </div>
        
        <!-- Aether Core / Center -->
        <div class="w-72 h-72 rounded-full border-4 border-[#cc785c] bg-[#cc785c]/5 flex flex-col items-center justify-center z-10 relative">
          <h2 class="text-xl font-black text-[#cc785c] text-center px-4 leading-tight">
            AETHER<br>GATEWAY
          </h2>
          <div class="mt-4 text-xs font-mono text-center leading-relaxed">
            Auth<br>Quota<br>Load Balance
          </div>
        </div>

        <!-- Ingress Left Arc -->
        <div class="absolute left-24 top-1/2 -translate-y-1/2 flex flex-col gap-6">
          <div class="w-40 h-10 bg-white dark:bg-[#1e1c19] border border-[#e5e4df] dark:border-white/10 rounded pl-4 flex items-center relative">
            <span class="font-mono text-sm font-bold">Claude API</span>
            <div class="absolute right-[-40px] top-1/2 h-[1px] w-10 bg-[#cc785c]" />
          </div>
          <div class="w-40 h-10 bg-white dark:bg-[#1e1c19] border border-[#e5e4df] dark:border-white/10 rounded pl-4 flex items-center relative">
            <span class="font-mono text-sm font-bold">OpenAI API</span>
            <div class="absolute right-[-40px] top-1/2 h-[1px] w-10 bg-[#cc785c]" />
          </div>
          <div class="w-40 h-10 bg-white dark:bg-[#1e1c19] border border-[#e5e4df] dark:border-white/10 rounded pl-4 flex items-center relative">
            <span class="font-mono text-sm font-bold">Gemini API</span>
            <div class="absolute right-[-40px] top-1/2 h-[1px] w-10 bg-[#cc785c]" />
          </div>
        </div>

        <!-- Engines Top/Bottom -->
        <div class="absolute top-24 left-1/2 -translate-x-1/2 flex flex-col items-center">
          <div class="w-48 h-16 bg-[#a855f7]/10 border-2 border-[#a855f7] rounded-xl flex items-center justify-center text-sm font-bold text-[#a855f7]">
            同格式透传管道
          </div>
          <div class="w-[1px] h-12 bg-[#a855f7]" />
        </div>

        <div class="absolute bottom-24 left-1/2 -translate-x-1/2 flex flex-col items-center">
          <div class="w-[1px] h-12 bg-[#3b82f6]" />
          <div class="w-48 h-16 bg-[#3b82f6]/10 border-2 border-[#3b82f6] rounded-xl flex items-center justify-center text-sm font-bold text-[#3b82f6]">
            原生直通管道
          </div>
        </div>

        <!-- Egress Right Arc -->
        <div class="absolute right-24 top-1/2 -translate-y-1/2 flex flex-col gap-4">
          <div class="w-48 h-12 border border-dashed border-emerald-500 rounded flex items-center justify-center relative bg-emerald-500/5 text-emerald-600 font-bold">
            <div class="absolute left-[-60px] top-1/2 h-[1px] w-14 bg-emerald-500/50" />
            Standard Providers
          </div>
          <div class="w-48 h-12 border border-dashed border-amber-500 rounded flex items-center justify-center relative bg-amber-500/5 text-amber-600 font-bold">
            <div class="absolute left-[-60px] top-1/2 h-[1px] w-14 bg-amber-500/50" />
            Reverse Proxies
          </div>
        </div>
      </div>

      <!-- Variant 3: 2.5D Stacked -->
      <div
        v-if="activeDemo === 3"
        class="w-full h-[800px] border border-dashed border-[#cc785c]/30 rounded-3xl relative pt-32 pb-12 flex justify-center perspective-[1000px]"
      >
        <div class="absolute top-4 left-6 text-sm font-mono text-[#cc785c]/50">
          Layout Variant: 2.5D Stacked Layers (AWS Style)
        </div>
        
        <div
          class="w-[600px] h-[600px] relative transition-transform duration-500 preserve-3d"
          style="transform: rotateX(55deg) rotateZ(-45deg);"
        >
          <!-- Layer 1: Ingress (Top) -->
          <div class="absolute inset-x-0 top-0 h-40 bg-white/40 dark:bg-black/40 backdrop-blur-md border border-white/50 dark:border-white/10 rounded-tr-3xl rounded-bl-3xl shadow-2xl flex flex-col justify-center px-12 transform translate-z-[120px]">
            <h3 class="text-sm font-bold text-[#cc785c] absolute top-4 left-4 tracking-widest">
              LAYER 1: INGRESS CLIENTS
            </h3>
            <div class="flex gap-4 w-full">
              <div class="flex-1 h-12 bg-white dark:bg-[#2a2a2a] rounded flex items-center justify-center font-mono font-bold shadow-sm">
                Claude
              </div>
              <div class="flex-1 h-12 bg-white dark:bg-[#2a2a2a] rounded flex items-center justify-center font-mono font-bold shadow-sm">
                OpenAI
              </div>
              <div class="flex-1 h-12 bg-white dark:bg-[#2a2a2a] rounded flex items-center justify-center font-mono font-bold shadow-sm">
                Gemini
              </div>
            </div>
            <!-- Data Flow drops to L2 -->
            <div class="absolute bottom-[-60px] left-1/2 w-2 h-16 bg-[#cc785c] opacity-50 blur-sm rounded-full transform -rotate-x-90 translate-y-8" />
          </div>

          <!-- Layer 2: Aether Gateway Core (Middle) -->
          <div class="absolute inset-x-0 top-[200px] h-48 bg-[#cc785c]/20 backdrop-blur-xl border-2 border-[#cc785c] rounded-lg shadow-2xl flex items-center justify-center relative transform translate-z-[60px]">
            <h3 class="text-sm font-bold text-[#cc785c] absolute top-4 left-4 tracking-widest">
              LAYER 2: AETHER GATEWAY (CORE)
            </h3>
            <div class="text-center">
              <h2 class="text-3xl font-black text-[#cc785c] tracking-[0.3em] mb-4">
                AETHER
              </h2>
              <div class="flex gap-2">
                <span class="px-2 py-1 text-[10px] bg-white/50 dark:bg-black/50 rounded ring-1 ring-[#cc785c]">Auth</span>
                <span class="px-2 py-1 text-[10px] bg-white/50 dark:bg-black/50 rounded ring-1 ring-[#cc785c]">Rate Limit</span>
                <span class="px-2 py-1 text-[10px] bg-white/50 dark:bg-black/50 rounded ring-1 ring-[#cc785c]">Load Balance</span>
              </div>
            </div>
          </div>

          <!-- Layer 3: Egress & Providers (Bottom) -->
          <div class="absolute inset-x-0 bottom-[-50px] h-60 bg-[#f5f5f0]/50 dark:bg-[#1a1815]/50 backdrop-blur border border-[#e5e4df] dark:border-white/10 rounded-br-3xl rounded-tl-3xl shadow-xl flex flex-col p-8 transform translate-z-[0px]">
            <h3 class="text-sm font-bold text-[#666663] dark:text-[#a3a094] mb-4">
              LAYER 3: EGRESS & UPSTREAM
            </h3>
            
            <div class="flex mb-6">
              <div class="w-full h-16 bg-[#3b82f6]/20 border border-[#3b82f6] rounded flex items-center justify-center text-[#3b82f6] font-bold">
                Direct Passthrough
              </div>
            </div>

            <div class="w-full flex gap-4 h-16">
              <div class="w-full border border-dashed border-emerald-500 rounded bg-emerald-500/10 flex items-center justify-center text-emerald-600 font-bold">
                Custom providers
              </div>
              <div class="w-full border border-dashed border-amber-500 rounded bg-amber-500/10 flex items-center justify-center text-amber-600 font-bold">
                Custom endpoints
              </div>
            </div>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.perspective-\[1000px\] {
  perspective: 1000px;
}
.preserve-3d {
  transform-style: preserve-3d;
}
.translate-z-\[120px\] { transform: translateZ(120px); }
.translate-z-\[60px\] { transform: translateZ(60px); }
.translate-z-\[0px\] { transform: translateZ(0px); }
.-rotate-x-90 { transform: rotateX(-90deg); }
</style>
