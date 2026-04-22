<template>
  <div class="gallery" :style="gridStyle">
    <div
      v-for="(item, index) in images"
      :key="index"
      class="card"
      @click="openPreview(item.url)"
    >
      <div class="img-wrap">
        <img :src="item.url" :alt="item.title" loading="lazy" />
      </div>
      <div class="title">{{ item.title }}</div>
    </div>
  </div>

  <!-- 预览层 -->
  <div v-if="previewVisible" class="preview" @click="closePreview">
    <img :src="previewUrl" />
  </div>
</template>

<script setup lang="ts">
import { computed, ref } from 'vue'

interface ImageItem {
  url: string
  title: string
}

const props = defineProps<{
  columnWidth: number
  gap?: number
  images: ImageItem[]
}>()

const gap = props.gap ?? 16

const previewVisible = ref(false)
const previewUrl = ref('')

function openPreview(url: string) {
  previewUrl.value = url
  previewVisible.value = true
}

function closePreview() {
  previewVisible.value = false
}

// 行优先（默认就是 row）
const gridStyle = computed(() => ({
  display: 'grid',
  gridAutoFlow: 'row',
  gridTemplateColumns: `repeat(auto-fill, minmax(${props.columnWidth}px, 1fr))`,
  gap: `${gap}px`,
  alignItems: 'start'
}))
</script>

<style scoped>
/* 容器 */
.gallery {
  padding: 4px;
}

/* 卡片 */
.card {
  background: var(--card-bg);
  border-radius: 16px;
  overflow: hidden;
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.06);
  cursor: pointer;
  transition: transform 0.25s ease, box-shadow 0.25s ease;
}

.card:hover {
  transform: translateY(-6px);
  box-shadow: 0 12px 28px rgba(0, 0, 0, 0.12);
}

/* 图片区域 */
.img-wrap {
  width: 100%;
  aspect-ratio: 4 / 3;
  overflow: hidden;
  background: #f3f4f6;
}

.img-wrap img {
  width: 100%;
  height: 100%;
  object-fit: cover;
  display: block;
  transition: transform 0.35s ease;
}

.card:hover img {
  transform: scale(1.05);
}

/* 标题 */
.title {
  padding: 10px 12px 14px;
  font-size: 14px;
  text-align: center;
  color: var(--text-muted);
  line-height: 1.4;
}

/* 预览层 */
.preview {
  position: fixed;
  inset: 0;
  background: rgba(0, 0, 0, 0.85);
  display: flex;
  align-items: center;
  justify-content: center;
  z-index: 999;
}

.preview img {
  max-width: 92%;
  max-height: 92%;
  border-radius: 14px;
  box-shadow: 0 12px 40px rgba(0, 0, 0, 0.5);
  animation: zoomIn 0.25s ease;
}

@keyframes zoomIn {
  from {
    transform: scale(0.85);
    opacity: 0;
  }
  to {
    transform: scale(1);
    opacity: 1;
  }
}

/* ========== 主题变量（支持 html.dark） ========== */
:root {
  --card-bg: #ffffff;
  --text-muted: #6b7280;
}

:root html.dark, html.dark :root {
  --card-bg: #1f2937;
  --text-muted: #9ca3af;
}

/* 更稳妥：直接针对 html.dark 覆盖 */
html.dark .card {
  box-shadow: 0 6px 18px rgba(0, 0, 0, 0.4);
}

html.dark .img-wrap {
  background: #111827;
}
</style>