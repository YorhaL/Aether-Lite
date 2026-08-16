<script setup lang="ts">
import { ref, computed, onMounted, onUnmounted } from 'vue'
import { useDarkMode } from '@/composables/useDarkMode'

const { isDark } = useDarkMode()

const DESIGN_W = 760
const DESIGN_H = 420

const wrapperRef = ref<HTMLElement>()
const scale = ref(1)
const wrapperWidth = ref(DESIGN_W)

function updateScale() {
  if (!wrapperRef.value) return
  const w = wrapperRef.value.clientWidth
  wrapperWidth.value = w
  scale.value = Math.min(w / DESIGN_W, 1)
}

const offsetLeft = computed(() => {
  return Math.max(0, (wrapperWidth.value - DESIGN_W * scale.value) / 2)
})

let ro: ResizeObserver | null = null
onMounted(() => {
  updateScale()
  ro = new ResizeObserver(updateScale)
  if (wrapperRef.value) ro.observe(wrapperRef.value)
})
onUnmounted(() => ro?.disconnect())

const colors = computed(() => {
  const brand = '#cc7154'
  return {
    bg: isDark.value ? '#121212' : '#fcfcfc',
    cardBg: isDark.value ? '#1a1a1c' : '#ffffff',
    cardBorder: isDark.value ? '#2a2a2c' : '#e8e8ec',
    track: isDark.value ? 'rgba(255,255,255,0.08)' : '#e5e7eb',
    textMain: isDark.value ? '#f3f4f6' : '#1f2937',
    textSecondary: isDark.value ? '#9ca3af' : '#4b5563',
    textMuted: isDark.value ? '#6b7280' : '#8c94a1',
    brand,
    brandSoft: isDark.value ? 'rgba(204,113,84,0.12)' : 'rgba(204,113,84,0.06)',
    coreBorder: isDark.value ? 'rgba(204,113,84,0.4)' : 'rgba(204,113,84,0.25)',
  }
})

const shadows = computed(() => {
  return isDark.value
    ? { sm: '0 2px 4px -1px rgba(0,0,0,0.5)', lg: '0 8px 24px -8px rgba(0,0,0,0.7)' }
    : { sm: '0 2px 6px -2px rgba(0,0,0,0.04)', lg: '0 6px 20px -6px rgba(0,0,0,0.06)' }
})

/*
 * Coordinate system (all absolute px within the 760x420 container):
 *
 * Three columns at X = 188, 380, 572
 *
 * SOURCES label:          Y = 8
 * Source pills:           top=24, h=36  => bottom = 60
 *   Line: 3 merge to 1 at Gateway top Y=78, then straight to dashed box top Y=98
 * Gateway box:            top=78, h=240 => bottom = 318
 * Dashed box:             top=98, h=112 => bottom=210 (left=180, w=400)
 *
 * Junction dots (3 total, all at X=380):
 *   [dot] Y=78   Gateway top merge    (3 sources -> 1)
 *   [dot] Y=210  Route node           (1 -> 3 engines)
 *   [dot] Y=318  Merge/exit node      (3 engines -> 1 -> 3 outputs, at Gateway bottom)
 *
 * Engine cards:           top=250, h=28 => bottom=278
 *   Line: Y=278 stub Y=284, curve to merge/exit Y=318
 *   Line: Y=318 diverge to output pills top Y=360
 * OUTPUTS label:          Y = 342
 * Output pills:           top=360, h=36
 *
 * Engine card positions:
 *   w=170, left=103/295/487 => center=188/380/572
 *
 * Animated dots: 8 total (curves only, no straight-line anims)
 *   2x Sources->dashed box | 2x Route->Engines
 *   2x Engines->merge/exit | 2x exit->outputs
 */
</script>

<template>
  <div
    ref="wrapperRef"
    class="w-full overflow-hidden"
    :style="{ height: `${DESIGN_H * scale}px` }"
  >
    <div
      class="relative w-[760px] overflow-hidden rounded-[24px] border diagram-container transition-colors duration-300"
      :style="{
        backgroundColor: colors.bg,
        borderColor: colors.cardBorder,
        transform: `scale(${scale})`,
        transformOrigin: 'top left',
        marginLeft: `${offsetLeft}px`,
      }"
    >
      <div class="relative w-[760px] h-[420px]">
        <!-- SVG connection lines -->
        <svg
          class="absolute inset-0 w-full h-full pointer-events-none z-30"
          xmlns="http://www.w3.org/2000/svg"
        >
          <!-- Track lines -->
          <g
            :stroke="colors.track"
            stroke-width="1.5"
            fill="none"
            stroke-linecap="round"
          >
            <!-- Sources (Y=60) -> merge at Gateway top (Y=78) -> dashed box top (Y=98) -->
            <path
              id="f-in-1"
              d="M 188 60 L 188 66 C 188 72, 380 72, 380 78 L 380 98"
            />
            <path
              id="f-in-2"
              d="M 380 60 L 380 98"
            />
            <path
              id="f-in-3"
              d="M 572 60 L 572 66 C 572 72, 380 72, 380 78 L 380 98"
            />
            <!-- Route dot (Y=210) -> Engine cards top (Y=250) -->
            <path
              id="f-sL"
              d="M 380 210 C 380 230, 188 230, 188 250"
            />
            <path
              id="f-sM"
              d="M 380 210 L 380 250"
            />
            <path
              id="f-sR"
              d="M 380 210 C 380 230, 572 230, 572 250"
            />
            <!-- Engine bottom (Y=278) -> merge/exit node at Gateway bottom (Y=318) -->
            <path
              id="f-oL"
              d="M 188 278 L 188 284 C 188 302, 380 302, 380 318"
            />
            <path
              id="f-oM"
              d="M 380 278 L 380 318"
            />
            <path
              id="f-oR"
              d="M 572 278 L 572 284 C 572 302, 380 302, 380 318"
            />
            <!-- Merge/exit node at Gateway bottom (Y=318) -> output pills top (Y=360) -->
            <path
              id="f-oL2"
              d="M 380 318 C 380 338, 188 338, 188 360"
            />
            <path
              id="f-oM2"
              d="M 380 318 L 380 360"
            />
            <path
              id="f-oR2"
              d="M 380 318 C 380 338, 572 338, 572 360"
            />
          </g>
          <!-- Animated dots -->
          <g>
            <!-- Sources -> dashed box top (curves only, ~230px each, ~100px/s) -->
            <circle
              r="2.5"
              :fill="colors.brand"
            ><animateMotion
              dur="2.2s"
              repeatCount="indefinite"
            ><mpath href="#f-in-1" /></animateMotion></circle>
            <circle
              r="2.5"
              :fill="colors.brand"
            ><animateMotion
              dur="2.2s"
              repeatCount="indefinite"
            ><mpath href="#f-in-3" /></animateMotion></circle>
            <!-- Route -> Engines (curves only, ~250px each) -->
            <circle
              r="2.5"
              :fill="colors.brand"
            ><animateMotion
              dur="1.4s"
              repeatCount="indefinite"
            ><mpath href="#f-sL" /></animateMotion></circle>
            <circle
              r="2.5"
              :fill="colors.brand"
            ><animateMotion
              dur="1.4s"
              repeatCount="indefinite"
            ><mpath href="#f-sR" /></animateMotion></circle>
            <!-- Engines -> merge/exit node (curves only, ~230px each) -->
            <circle
              r="2.5"
              :fill="colors.brand"
            ><animateMotion
              dur="1.4s"
              repeatCount="indefinite"
            ><mpath href="#f-oL" /></animateMotion></circle>
            <circle
              r="2.5"
              :fill="colors.brand"
            ><animateMotion
              dur="1.4s"
              repeatCount="indefinite"
            ><mpath href="#f-oR" /></animateMotion></circle>
            <!-- Exit -> outputs (curves only, ~210px each) -->
            <circle
              r="2.5"
              :fill="colors.brand"
            ><animateMotion
              dur="1.2s"
              repeatCount="indefinite"
            ><mpath href="#f-oL2" /></animateMotion></circle>
            <circle
              r="2.5"
              :fill="colors.brand"
            ><animateMotion
              dur="1.2s"
              repeatCount="indefinite"
            ><mpath href="#f-oR2" /></animateMotion></circle>
          </g>
        </svg>

        <!-- SOURCES label -->
        <div class="absolute w-full top-[8px] flex justify-center items-center gap-1.5 z-10">
          <div
            class="w-1 h-1 rounded-full"
            :style="{ backgroundColor: colors.textMuted }"
          />
          <span
            class="font-sans text-[9px] font-extrabold tracking-[0.2em] uppercase"
            :style="{ color: colors.textMuted }"
          >SOURCES</span>
        </div>

        <!-- Source pills: top=24, h=36, centerY=42 -->
        <!-- Claude:  center X=188, w=100 => left=138 -->
        <!-- OpenAI:  center X=380, w=100 => left=330 -->
        <!-- Gemini:  center X=572, w=100 => left=522 -->
        <div class="absolute w-full top-[24px] z-10">
          <div
            class="absolute left-[138px] w-[100px] h-[36px] rounded-full flex items-center justify-center gap-2 "
            :style="{ backgroundColor: colors.cardBg, border: `1px solid ${colors.cardBorder}`, boxShadow: shadows.sm }"
          >
            <div
              class="w-1.5 h-1.5 rounded-full"
              :style="{ backgroundColor: colors.brand }"
            />
            <span
              class="font-sans text-[11px] font-bold"
              :style="{ color: colors.textMain }"
            >Claude</span>
          </div>
          <div
            class="absolute left-[330px] w-[100px] h-[36px] rounded-full flex items-center justify-center gap-2 "
            :style="{ backgroundColor: colors.cardBg, border: `1px solid ${colors.cardBorder}`, boxShadow: shadows.sm }"
          >
            <div
              class="w-1.5 h-1.5 rounded-full"
              :style="{ backgroundColor: colors.brand }"
            />
            <span
              class="font-sans text-[11px] font-bold"
              :style="{ color: colors.textMain }"
            >OpenAI</span>
          </div>
          <div
            class="absolute left-[522px] w-[100px] h-[36px] rounded-full flex items-center justify-center gap-2 "
            :style="{ backgroundColor: colors.cardBg, border: `1px solid ${colors.cardBorder}`, boxShadow: shadows.sm }"
          >
            <div
              class="w-1.5 h-1.5 rounded-full"
              :style="{ backgroundColor: colors.brand }"
            />
            <span
              class="font-sans text-[11px] font-bold"
              :style="{ color: colors.textMain }"
            >Gemini</span>
          </div>
        </div>

        <!-- AETHER GATEWAY: top=78, h=240, w=600, left=80 -->
        <div
          class="absolute left-[80px] top-[78px] w-[600px] h-[240px] rounded-[24px] z-20"
          :style="{ backgroundColor: colors.cardBg, border: `1px solid ${colors.coreBorder}`, boxShadow: shadows.lg }"
        >
          <div
            class="absolute left-0 top-1/2 -translate-y-1/2 -translate-x-1/2 flex flex-col items-center gap-px px-[4px] py-1.5 rounded-full font-sans text-[7px] font-black leading-none"
            :style="{ backgroundColor: colors.cardBg, color: colors.brand }"
          >
            <span>A</span><span>E</span><span>T</span><span>H</span><span>E</span><span>R</span>
          </div>
        </div>

        <!-- Dashed box: abs positioned in outer 760x420 container -->
        <!-- center X=380, w=400 => left=180. top=98, h=112 => bottom=210 -->
        <div
          class="absolute left-[180px] top-[98px] w-[400px] h-[112px] rounded-2xl flex flex-col items-center gap-1.5 px-5 py-[10px] z-20"
          :style="{ border: `1.5px dashed ${colors.coreBorder}` }"
        >
          <div
            class="w-full h-[26px] rounded-xl flex items-center justify-center font-sans text-[10px] font-semibold"
            :style="{ border: `1px solid ${colors.cardBorder}`, color: colors.textMain }"
          >
            统一模型规范 / 协议聚合
          </div>
          <div class="w-full flex gap-1.5">
            <div
              class="flex-1 rounded-full flex items-center justify-center h-[26px] font-sans font-semibold text-[10px]"
              :style="{ backgroundColor: colors.brandSoft, color: colors.textMain }"
            >
              多端鉴权
            </div>
            <div
              class="flex-1 rounded-full flex items-center justify-center h-[26px] font-sans font-semibold text-[10px]"
              :style="{ backgroundColor: colors.brandSoft, color: colors.textMain }"
            >
              配额管控
            </div>
            <div
              class="flex-1 rounded-full flex items-center justify-center h-[26px] font-sans font-semibold text-[10px]"
              :style="{ backgroundColor: colors.brandSoft, color: colors.textMain }"
            >
              全局并发
            </div>
            <div
              class="flex-1 rounded-full flex items-center justify-center h-[26px] font-sans font-semibold text-[10px]"
              :style="{ backgroundColor: colors.brandSoft, color: colors.textMain }"
            >
              缓存亲和
            </div>
          </div>
          <div
            class="w-full h-[28px] rounded-xl flex items-center justify-center font-sans text-[10px] font-semibold"
            :style="{ border: `1px solid ${colors.cardBorder}`, color: colors.textMain }"
          >
            智能调度 / 故障转移
          </div>
        </div>

        <!-- Engine cards: bar style matching feature bars, w=170, h=28 -->
        <div
          class="absolute left-[103px] top-[250px] w-[170px] h-[28px] rounded-full flex items-center justify-center gap-1.5 z-20"
          :style="{ backgroundColor: colors.brandSoft, border: `1px solid ${colors.cardBorder}` }"
        >
          <div
            class="w-1.5 h-1.5 rounded-full"
            :style="{ backgroundColor: colors.brand }"
          />
          <span
            class="font-sans text-[10px] font-bold"
            :style="{ color: colors.textMain }"
          >同格式透传</span>
        </div>
        <div
          class="absolute left-[295px] top-[250px] w-[170px] h-[28px] rounded-full flex items-center justify-center gap-1.5 z-20"
          :style="{ backgroundColor: colors.brandSoft, border: `1px solid ${colors.cardBorder}` }"
        >
          <div
            class="w-1.5 h-1.5 rounded-full"
            :style="{ backgroundColor: colors.brand }"
          />
          <span
            class="font-sans text-[10px] font-bold"
            :style="{ color: colors.textMain }"
          >额度管控</span>
        </div>
        <div
          class="absolute left-[487px] top-[250px] w-[170px] h-[28px] rounded-full flex items-center justify-center gap-1.5 z-20"
          :style="{ backgroundColor: colors.brandSoft, border: `1px solid ${colors.cardBorder}` }"
        >
          <div
            class="w-1.5 h-1.5 rounded-full"
            :style="{ backgroundColor: colors.brand }"
          />
          <span
            class="font-sans text-[10px] font-bold"
            :style="{ color: colors.textMain }"
          >原生直通</span>
        </div>

        <!-- Junction dots (z-40, above SVG lines) -->
        <!-- Gateway top merge: center (380, 78) — 3 sources merge here -->
        <div
          class="absolute left-[376px] top-[74px] w-2 h-2 rounded-full border border-white dark:border-[#1e1e1e] z-40"
          :style="{ backgroundColor: colors.brand }"
        />
        <!-- Route node: center (380, 210) — 1 diverges to 3 engines -->
        <div
          class="absolute left-[376px] top-[206px] w-2 h-2 rounded-full border border-white dark:border-[#1e1e1e] z-40"
          :style="{ backgroundColor: colors.brand }"
        />
        <!-- Merge/exit node: center (380, 318) at Gateway bottom border -->
        <div
          class="absolute left-[376px] top-[314px] w-2 h-2 rounded-full border border-white dark:border-[#1e1e1e] z-40"
          :style="{ backgroundColor: colors.brand }"
        />

        <!-- OUTPUTS label -->
        <div class="absolute w-full top-[342px] flex justify-center items-center gap-1.5 z-10">
          <div
            class="w-1 h-1 rounded-full"
            :style="{ backgroundColor: colors.textMuted }"
          />
          <span
            class="font-sans text-[9px] font-extrabold tracking-[0.2em] uppercase"
            :style="{ color: colors.textMuted }"
          >OUTPUTS</span>
        </div>

        <!-- Output pills: top=360, h=36 -->
        <!-- Claude:  center X=188, w=120 => left=128 -->
        <!-- OpenAI:  center X=380, w=120 => left=320 -->
        <!-- Gemini:  center X=572, w=120 => left=512 -->
        <div class="absolute w-full top-[360px] z-10">
          <div
            class="absolute left-[128px] w-[120px] h-[36px] rounded-full flex items-center justify-center gap-2"
            :style="{ backgroundColor: colors.cardBg, border: `1px solid ${colors.cardBorder}`, boxShadow: shadows.sm }"
          >
            <div
              class="w-1.5 h-1.5 rounded-full"
              :style="{ backgroundColor: colors.brand }"
            />
            <span
              class="font-sans text-[10px] font-bold"
              :style="{ color: colors.textMain }"
            >Claude 响应</span>
          </div>
          <div
            class="absolute left-[320px] w-[120px] h-[36px] rounded-full flex items-center justify-center gap-2"
            :style="{ backgroundColor: colors.cardBg, border: `1px solid ${colors.cardBorder}`, boxShadow: shadows.sm }"
          >
            <div
              class="w-1.5 h-1.5 rounded-full"
              :style="{ backgroundColor: colors.brand }"
            />
            <span
              class="font-sans text-[10px] font-bold"
              :style="{ color: colors.textMain }"
            >OpenAI 响应</span>
          </div>
          <div
            class="absolute left-[512px] w-[120px] h-[36px] rounded-full flex items-center justify-center gap-2"
            :style="{ backgroundColor: colors.cardBg, border: `1px solid ${colors.cardBorder}`, boxShadow: shadows.sm }"
          >
            <div
              class="w-1.5 h-1.5 rounded-full"
              :style="{ backgroundColor: colors.brand }"
            />
            <span
              class="font-sans text-[10px] font-bold"
              :style="{ color: colors.textMain }"
            >Gemini 响应</span>
          </div>
        </div>
      </div>
    </div>
  </div>
</template>

<style scoped>
.font-sans { font-family: 'Inter', system-ui, -apple-system, sans-serif; }
</style>
