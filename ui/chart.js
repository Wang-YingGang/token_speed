export function speedScale(values) {
  const maximum = Math.max(0, ...values.filter(Number.isFinite));
  const target = (maximum || 1) / 2;
  const magnitude = 10 ** Math.floor(Math.log10(target));
  const step = [1, 2, 2.5, 5, 10].find((n) => n * magnitude >= target) * magnitude;
  return { maximum: step * 2, step, ticks: [0, step, step * 2] };
}

export function tickLabel(value) {
  if (value >= 1e6) return `${Number((value / 1e6).toFixed(1))}m`;
  if (value >= 1e3) return `${Number((value / 1e3).toFixed(1))}k`;
  return String(Number(value.toPrecision(3)));
}

export function speedChart(app, width = 40) {
  const ns = "http://www.w3.org/2000/svg";
  const svg = document.createElementNS(ns, "svg");
  svg.setAttribute("class", "taskbar-sparkline");
  svg.setAttribute("viewBox", `0 0 ${width} 20`);
  svg.setAttribute("preserveAspectRatio", "none");
  const records = app.history.slice(0, 20).reverse();
  const speeds = records.map((r) => Math.max(0, Number.isFinite(r.outputTps) ? r.outputTps : 0));
  const scale = speedScale(speeds);
  const labelWidth = Math.max(...scale.ticks.map((v) => tickLabel(v).length)) * 3.5 + 2;
  const left = Math.min(labelWidth + 1, width - 12);
  const right = width - 1;
  const y = (value) => 17 - value / scale.maximum * 13;
  svg.setAttribute("aria-label", `${app.appType} 最近 ${records.length} 轮，Y 轴 0–${scale.maximum} t/s，每格 ${scale.step} t/s`);
  const title = document.createElementNS(ns, "title");
  title.textContent = `最近 ${records.length} 轮完成速度 · Y 轴每格 ${scale.step} t/s · 横轴由旧到新`;
  svg.append(title);
  for (const value of scale.ticks) {
    const line = document.createElementNS(ns, "line");
    line.setAttribute("x1", left); line.setAttribute("x2", right);
    line.setAttribute("y1", y(value)); line.setAttribute("y2", y(value));
    line.setAttribute("class", "chart-grid");
    svg.append(line);
    const label = document.createElementNS(ns, "text");
    label.setAttribute("x", left - 2); label.setAttribute("y", y(value) + 2);
    label.setAttribute("text-anchor", "end"); label.setAttribute("class", "chart-tick");
    label.textContent = tickLabel(value); svg.append(label);
  }
  const axis = document.createElementNS(ns, "path");
  axis.setAttribute("d", `M ${left} 3 V 17 H ${right}`); axis.setAttribute("class", "chart-axis");
  svg.append(axis);
  if (records.length) {
    const points = speeds.map((speed, i) => [records.length === 1 ? (left+right)/2 : left + i/(records.length-1)*(right-left), y(speed)]);
    const line = document.createElementNS(ns, "polyline");
    line.setAttribute("points", points.map(([x,y]) => `${x.toFixed(2)},${y.toFixed(2)}`).join(" "));
    line.setAttribute("class", "chart-line");
    const dot = document.createElementNS(ns, "circle");
    dot.setAttribute("cx", points.at(-1)[0]); dot.setAttribute("cy", points.at(-1)[1]); dot.setAttribute("r", ".9");
    dot.setAttribute("class", "chart-point");
    svg.append(line,dot);
  }
  return svg;
}
