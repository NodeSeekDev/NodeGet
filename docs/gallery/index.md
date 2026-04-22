---
layout: home
---

<h1 style="margin:16px auto; font-size:1.4rem">展示画廊</h1>


<Waterfall :images="images" :columnWidth="260"></Waterfall>
<Waterfall :column-width="280" :items="images" />

<script setup lang="ts">
import { ref } from 'vue';
import Waterfall from '../components/waterfall.vue'

const images = ref([
]);
</script>