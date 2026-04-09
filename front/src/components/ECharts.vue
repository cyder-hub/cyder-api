<script setup lang="ts">
import { nextTick, onBeforeUnmount, onMounted, ref, watch } from "vue";
import { use } from "echarts/core";
import { CanvasRenderer } from "echarts/renderers";
import { BarChart, LineChart } from "echarts/charts";
import {
  TitleComponent,
  TooltipComponent,
  LegendComponent,
  GridComponent,
  ToolboxComponent,
  DataZoomComponent,
} from "echarts/components";
import VChart from "vue-echarts";
import type { EChartsOption } from "echarts";

// Register the necessary components
use([
  CanvasRenderer,
  BarChart,
  LineChart,
  TitleComponent,
  TooltipComponent,
  LegendComponent,
  GridComponent,
  ToolboxComponent,
  DataZoomComponent,
]);

// Define props
interface EChartsProps {
  option: EChartsOption;
  class?: string;
  style?: any;
}

const props = defineProps<EChartsProps>();
const chartRef = ref<InstanceType<typeof VChart> | null>(null);

const resizeChart = async () => {
  await nextTick();
  chartRef.value?.resize?.();
};

onMounted(() => {
  resizeChart();
  window.addEventListener("resize", resizeChart, { passive: true });
  window.addEventListener("orientationchange", resizeChart, { passive: true });
});

onBeforeUnmount(() => {
  window.removeEventListener("resize", resizeChart);
  window.removeEventListener("orientationchange", resizeChart);
});

watch(
  () => props.option,
  () => {
    resizeChart();
  },
  { deep: true },
);
</script>

<template>
  <v-chart
    ref="chartRef"
    :option="props.option"
    :class="props.class"
    :style="props.style"
    autoresize
  />
</template>
