import assert from "node:assert/strict";
import { ActivityTracker } from "../ui/activity.js";
import { speedScale, tickLabel } from "../ui/chart.js";
let count = 0;
function test(name, run) { run(); count++; console.log(`✓ ${name}`); }
const now = 1_800_000_000_000;
const app = (id, time = now - 60_000) => ({ appType:"claude",latest:id ? { requestId:id,createdAt:time/1000 } : null });
test("启动时历史记录显示待机", () => {
  const a=new ActivityTracker(); a.update({apps:[app("old")]},now); assert.equal(a.status(app("old"),15,now),"idle");
});
test("启动时真正近期记录保留剩余时间", () => {
  const a=new ActivityTracker(); const x=app("recent",now-5000); a.update({apps:[x]},now);
  assert.equal(a.status(x,15,now),"recent"); assert.equal(a.status(x,15,now+10_000),"idle");
});
test("新完成记录即时显示，计时到期回到待机", () => {
  const a=new ActivityTracker(); a.update({apps:[app("old")]},now); const x=app("new"); a.update({apps:[x]},now+1000);
  assert.equal(a.status(x,15,now+15_999),"recent"); assert.equal(a.status(x,15,now+16_000),"idle");
});
test("重复事件不会持续续期", () => {
  const a=new ActivityTracker(); const x=app("old"); a.update({apps:[x]},now); a.update({apps:[x]},now+30_000);
  assert.equal(a.status(x,15,now+30_000),"idle");
});
test("没有记录显示空状态，未来时间不会制造近期状态", () => {
  const a=new ActivityTracker(); const future=app("future",now+100_000); a.update({apps:[future]},now);
  assert.equal(a.status(future,15,now),"idle"); assert.equal(a.status(app(null),15,now),"empty");
});
test("各应用独立计时，可调整保留时间", () => {
  const a=new ActivityTracker(); const c=app("c",now-20_000); const x={...app("x",now-1000),appType:"codex"};
  a.update({apps:[c,x]},now); assert.equal(a.status(c,15,now),"idle"); assert.equal(a.status(c,30,now),"recent"); assert.equal(a.status(x,15,now),"recent");
});
test("速度刻度随范围自动缩放，始终覆盖所有点", () => {
  assert.deepEqual(speedScale([40,88.8]),{maximum:100,step:50,ticks:[0,50,100]});
  assert.deepEqual(speedScale([400,777]),{maximum:1000,step:500,ticks:[0,500,1000]});
  for(const max of [.001,.4,1,2.1,12,101,500,50000]) { const s=speedScale([max]); assert.ok(s.maximum>=max); assert.equal(s.ticks[1],s.step); }
});
test("空数据、无效速度和紧凑刻度标签", () => {
  assert.equal(speedScale([]).maximum,1); assert.equal(speedScale([NaN,Infinity]).maximum,1);
  assert.equal(tickLabel(1000),"1k"); assert.equal(tickLabel(.25),"0.25");
});
console.log(`全部 ${count} 项活动状态和坐标轴测试通过`);
