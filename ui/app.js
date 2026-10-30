import { ActivityTracker, recordTime } from "./activity.js";
import { speedChart } from "./chart.js";

const tauri = window.__TAURI__;
const isSettings = tauri?.window.getCurrentWindow().label === "settings";
const slot = document.querySelector("#display-slot");
const compactMain = document.querySelector("#compact-main");
const activity = new ActivityTracker();
let snapshot = { apps: [{ appType: "claude", latest: null, history: [] }, { appType: "codex", latest: null, history: [] }], databaseFound: false };
let config = { displayMode: "both", windowMode: "taskbar", showChart: true, chartWidth: 40, fontSize: 11, opacity: 92,
  alwaysOnTop: true, positionLocked: false, theme: "dark", idleAfterSeconds: 15, offset: 0 };
let geometry = null;
let renderKey = "";
let feedbackTimer;
let drag = null;
let floatingPointer = null;
let suppressClick = false;
let saveQueue = Promise.resolve();
const controls = { "display-mode": "displayMode", "window-mode": "windowMode", "show-chart": "showChart", "font-size": "fontSize",
  opacity: "opacity", "always-on-top": "alwaysOnTop", "position-locked": "positionLocked", theme: "theme",
  "idle-after-seconds": "idleAfterSeconds", "chart-width": "chartWidth", "taskbar-offset": "offset" };

document.querySelector("#monitor").hidden = isSettings;
document.querySelector("#settings-panel").hidden = !isSettings;
document.body.classList.toggle("settings-window", isSettings);

function formatSpeed(value) {
  if (!Number.isFinite(value)) return "—";
  return value >= 100 ? value.toFixed(0) : value >= 10 ? value.toFixed(1) : value.toFixed(2);
}
function recordDescription(app) {
  if (!app?.latest) return "暂无完成记录";
  const time = recordTime(app.latest);
  return `上次完成速度 ${formatSpeed(app.latest.outputTps)} t/s\n${time ? `记录时间 ${new Date(time).toLocaleString("zh-CN", { hour12:false })}` : "记录时间未知"}\n${app.latest.model || ""}`;
}
function renderMonitor() {
  if (isSettings) return;
  const types = config.displayMode === "both" ? ["claude", "codex"] : [config.displayMode];
  slot.replaceChildren(...types.map((type) => {
    const app = snapshot.apps.find((a) => a.appType === type);
    const status = activity.status(app, config.idleAfterSeconds);
    const row = document.createElement("span");
    row.className = `taskbar-row ${type} ${status === "recent" ? "is-recent" : "is-idle"}`;
    row.dataset.appType = type; row.dataset.status = status;
    const label = document.createElement("span"); label.className = "taskbar-app-label"; label.textContent = type === "codex" ? "Codex" : "Claude";
    const speed = document.createElement("span"); speed.className = "taskbar-speed";
    speed.textContent = status === "recent" ? formatSpeed(app.latest.outputTps) : status === "empty" ? "暂无" : "待机";
    if (status === "recent") { const unit = document.createElement("small"); unit.textContent = "t/s"; speed.append(unit); }
    if (config.displayMode === "both") row.append(label);
    row.append(speed);
    row.title = `${status === "recent" ? "刚收到完成记录" : status === "idle" ? "等待新记录" : "暂无数据"}\n${recordDescription(app)}\n完成速度不是实时生成状态 · 单击或右键设置`;
    if (config.showChart && app?.history?.length) row.append(speedChart(app, config.chartWidth));
    return row;
  }));
  slot.classList.toggle("single", types.length === 1);
  renderKey = snapshot.apps.map((app) => activity.status(app,config.idleAfterSeconds)).join();
}
function renderDatabase() {
  const status = document.querySelector("#database-status");
  const connected = snapshot.databaseFound && !snapshot.message;
  status.classList.toggle("connected",connected); status.classList.toggle("error",!connected);
  status.querySelector(".database-label").textContent = connected ? "数据库已连接" : snapshot.databaseFound ? "读取失败" : "等待数据库";
  status.title = snapshot.message || snapshot.databasePath || "CC Switch";
}
function applySnapshot(value) {
  if (!value?.apps) return;
  snapshot = value;
  if (value.databaseFound && !value.message) activity.update(value);
  renderMonitor(); renderDatabase();
}
async function refreshGeometry() {
  if (!tauri) return;
  try {
    geometry = await tauri.core.invoke("get_monitor_geometry");
    document.querySelector("#monitor-geometry").textContent = `X ${geometry.x}`;
    document.querySelector("#monitor-width").textContent = `${Math.round(geometry.width)} px`;
  } catch (error) { console.error("读取监控位置失败",error); }
}
function applySettings(value) {
  if (!value) return;
  config = { ...config, ...value };
  document.body.classList.toggle("floating-window", !isSettings && config.windowMode === "floating");
  document.body.classList.toggle("light-monitor", config.theme === "light");
  document.body.classList.toggle("position-locked",config.positionLocked);
  document.documentElement.style.setProperty("--monitor-font",`${config.fontSize}px`);
  document.documentElement.style.setProperty("--monitor-opacity",config.opacity/100);
  for (const [id,key] of Object.entries(controls)) {
    const control = document.getElementById(id);
    if (control.type === "checkbox") control.checked = Boolean(config[key]); else control.value = config[key];
  }
  for (const [id,value] of [["font-size",config.fontSize],["opacity",config.opacity],["chart-width",config.chartWidth]]) {
    document.getElementById(`${id}-value`).value = `${value}${id === "opacity" ? "%" : " px"}`;
  }
  document.querySelectorAll(".floating-setting input, .floating-setting select").forEach((e) => { e.disabled = config.windowMode !== "floating"; });
  document.querySelectorAll(".taskbar-setting input, .taskbar-setting button").forEach((e) => { e.disabled = config.windowMode !== "taskbar"; });
  document.querySelector("#chart-width").disabled = !config.showChart;
  document.querySelector("#floating-hint").textContent = config.windowMode === "floating" ? "当前已启用" : "悬浮模式可用";
  renderMonitor(); refreshGeometry();
}
function feedback(message, error = false) {
  const saveStatus = document.querySelector("#save-status");
  saveStatus.textContent = error ? "保存失败" : message === "位置已复位" ? "已复位" : "已保存";
  saveStatus.classList.remove("saving");
  if (!error) return;
  const element = document.querySelector("#feedback");
  element.textContent = message; element.classList.toggle("error",error); element.hidden = false;
  clearTimeout(feedbackTimer); feedbackTimer = setTimeout(() => { element.hidden = true; },error ? 6000 : 1300);
}
function save(patch) {
  document.querySelector("#save-status").textContent = "保存中";
  document.querySelector("#save-status").classList.add("saving");
  saveQueue = saveQueue.then(async () => {
    try { applySettings(await tauri?.core.invoke("set_monitor_settings",{patch}) ?? { ...config,...patch }); feedback("已保存"); }
    catch (error) { applySettings(config); feedback(`保存失败：${error}`,true); }
  });
  return saveQueue;
}
for (const [id,key] of Object.entries(controls)) {
  const control = document.getElementById(id);
  if (control.type === "range") control.addEventListener("input",() => {
    document.getElementById(`${id}-value`).value = `${control.value}${id === "opacity" ? "%" : " px"}`;
  });
  control.addEventListener("change",() => {
    const value = control.type === "checkbox" ? control.checked : ["range","number"].includes(control.type) || id === "idle-after-seconds" ? Number(control.value) : control.value;
    const patch = { [key]:value };
    if (key === "offset") patch.offset = Math.round(value);
    save(patch);
  });
}
document.querySelectorAll("[data-nudge]").forEach((button) => button.addEventListener("click",() => {
  const input = document.querySelector("#taskbar-offset");
  input.value = Math.max(-5000,Math.min(5000,Number(input.value)+Number(button.dataset.nudge)));
  input.dispatchEvent(new Event("change"));
}));
async function openSettings() { try { await tauri?.core.invoke("show_settings"); refreshGeometry(); } catch (error) { console.error(error); } }
async function closeSettings() { await saveQueue; await tauri?.core.invoke("hide_settings"); }
document.querySelector("#settings-button").addEventListener("click",openSettings);
document.querySelector("#close-settings").addEventListener("click",closeSettings);
document.querySelector("#quit-button").addEventListener("click",async () => { await saveQueue; await tauri?.core.invoke("quit_app"); });
function setPage(page) {
  for (const name of ["display", "preferences"]) {
    const active = name === page;
    document.getElementById(`${name}-page`).hidden = !active;
    const tab = document.getElementById(`${name}-tab`);
    tab.classList.toggle("active",active); tab.setAttribute("aria-pressed",String(active));
  }
  document.querySelector(".settings-content").scrollTop = 0;
}
for (const name of ["display", "preferences"]) {
  const tab = document.getElementById(`${name}-tab`);
  tab.addEventListener("click",() => setPage(name));
  tab.addEventListener("keydown",(event) => {
    if (["ArrowLeft","ArrowRight"].includes(event.key)) {
      event.preventDefault(); const other = name === "display" ? "preferences" : "display";
      setPage(other); document.getElementById(`${other}-tab`).focus();
    }
  });
}
compactMain.addEventListener("click",(event) => {
  if (suppressClick) { suppressClick = false; event.preventDefault(); return; }
  openSettings();
});
compactMain.addEventListener("contextmenu",(event) => { event.preventDefault(); openSettings(); });
document.addEventListener("keydown",(event) => { if (event.key === "Escape" && isSettings) closeSettings(); });
compactMain.addEventListener("pointerdown",(event) => {
  suppressClick = false;
  if (isSettings || event.button !== 0 || config.positionLocked) return;
  if (config.windowMode === "floating") { floatingPointer = {x:event.clientX,y:event.clientY}; return; }
  const offset = config.offset;
  drag = { pointerId:event.pointerId,startX:event.screenX,startOffset:offset,offset,moved:false };
  compactMain.setPointerCapture(event.pointerId);
});
compactMain.addEventListener("pointermove",(event) => {
  if (floatingPointer && event.buttons === 1 && Math.hypot(event.clientX-floatingPointer.x,event.clientY-floatingPointer.y) >= 4) {
    floatingPointer = null; suppressClick = true;
    tauri?.core.invoke("start_monitor_drag").catch(console.error);
  }
  if (!drag || drag.pointerId !== event.pointerId) return;
  const distance = event.screenX-drag.startX;
  if (Math.abs(distance) < 4 && !drag.moved) return;
  drag.moved = true; drag.offset = Math.round(drag.startOffset+distance);
  tauri?.core.invoke("set_taskbar_offset",{offset:drag.offset,persist:false}).catch(console.error);
});
async function endDrag(event,persist) {
  floatingPointer = null;
  if (!drag || drag.pointerId !== event.pointerId) return;
  const finished = drag; drag = null;
  if (compactMain.hasPointerCapture(event.pointerId)) compactMain.releasePointerCapture(event.pointerId);
  if (!finished.moved) return;
  suppressClick = true;
  try { applySettings(await tauri?.core.invoke("set_taskbar_offset",{offset:finished.offset,persist})); }
  catch (error) { console.error(error); }
}
compactMain.addEventListener("pointerup",(event) => endDrag(event,true));
compactMain.addEventListener("pointercancel",(event) => endDrag(event,false));
compactMain.addEventListener("pointerleave",() => { floatingPointer = null; });
document.querySelector("#reset-position").addEventListener("click",async () => {
  try { applySettings(await tauri?.core.invoke("reset_monitor_position")); feedback("位置已复位"); }
  catch (error) { feedback(`复位失败：${error}`,true); }
});
const autostart = document.querySelector("#autostart-toggle");
autostart.addEventListener("change",async () => {
  const enabled = autostart.checked; autostart.disabled = true;
  try { await tauri?.core.invoke("set_autostart",{enabled}); }
  catch (error) { autostart.checked = !enabled; feedback(`保存失败：${error}`,true); }
  finally { autostart.disabled = false; }
});
setInterval(() => {
  if (isSettings) return;
  const key = snapshot.apps.map((app) => activity.status(app,config.idleAfterSeconds)).join();
  if (key !== renderKey) renderMonitor();
},500);
async function initialize() {
  renderMonitor(); renderDatabase();
  if (!tauri) return;
  await tauri.event.listen("usage-changed",(event) => applySnapshot(event.payload));
  await tauri.event.listen("taskbar-settings",(event) => applySettings(event.payload));
  await tauri.event.listen("activity-tick",() => {
    if (!isSettings && snapshot.apps.map((app) => activity.status(app,config.idleAfterSeconds)).join() !== renderKey) renderMonitor();
  });
  // Refresh numeric geometry each time a hidden settings window is reopened.
  await tauri.window.getCurrentWindow().onFocusChanged(({payload}) => { if (payload && isSettings) refreshGeometry(); });
  applySettings(await tauri.core.invoke("get_taskbar_settings"));
  applySnapshot(await tauri.core.invoke("get_snapshot"));
  autostart.checked = await tauri.core.invoke("get_autostart");
}
initialize().catch((error) => { console.error("初始化失败",error); if (isSettings) feedback(String(error),true); });
