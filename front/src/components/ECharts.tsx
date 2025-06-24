import { onMount, onCleanup, createEffect, Accessor } from 'solid-js';
import * as echarts from 'echarts/core';
import type { EChartsOption, ECharts } from 'echarts';

interface EChartsProps {
  options: Accessor<EChartsOption>;
  class?: string;
  style?: any;
}

export default function ECharts(props: EChartsProps) {
  let chartContainer: HTMLDivElement | undefined;
  let chartInstance: ECharts | undefined;

  onMount(() => {
    if (chartContainer) {
      chartInstance = echarts.init(chartContainer);

      const resizeObserver = new ResizeObserver(() => {
        chartInstance?.resize();
      });
      resizeObserver.observe(chartContainer);

      const handleWindowResize = () => chartInstance?.resize();
      window.addEventListener('resize', handleWindowResize);

      onCleanup(() => {
        resizeObserver.disconnect();
        window.removeEventListener('resize', handleWindowResize);
        chartInstance?.dispose();
      });
    }
  });

  createEffect(() => {
    if (chartInstance) {
      chartInstance.setOption(props.options(), { notMerge: true });
    }
  });

  return <div ref={chartContainer} class={props.class} style={props.style} />;
}
