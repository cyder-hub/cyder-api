<script setup lang="ts">
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
</script>

<template>
  <v-chart
    :option="props.option"
    :class="props.class"
    :style="props.style"
    autoresize
  />
</template>
