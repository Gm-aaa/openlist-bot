// Dependency-free regression tests for asynchronous UI ownership.
// Run: node --test tests/web-ui.test.mjs
import { readFileSync } from "node:fs";
import vm from "node:vm";
import test from "node:test";
import assert from "node:assert/strict";

const source = readFileSync(new URL("../src/web/app.js", import.meta.url), "utf8");
class Element {
  hidden = false;
  open = false;
  children = [];
  handlers = new Map();
  textContent = "";
  value = "";
  classList = { add() {} };
  addEventListener(name, fn) {
    this.handlers.set(name, [...(this.handlers.get(name) || []), fn]);
  }
  append(...nodes) { this.children.push(...nodes); }
  replaceChildren(...nodes) { this.children = nodes; }
  setAttribute() {}
  removeAttribute() {}
  focus() {}
  showModal() { this.open = true; }
  close() {
    this.open = false;
    for (const fn of this.handlers.get("close") || []) fn();
  }
}
function deferred() {
  let resolve;
  const promise = new Promise(r => { resolve = r; });
  return { promise, resolve };
}
async function page() {
  const nodes = new Map();
  const get = id => {
    if (!nodes.has(id)) nodes.set(id, new Element());
    return nodes.get(id);
  };
  const context = vm.createContext({
    document: { getElementById: get, querySelectorAll: () => [], createElement: () => new Element() },
    fetch: async () => ({ok: true, status: 200, json: async () => ({username: "admin", secondary_required: true})}),
    clearInterval() {}, setInterval() { return 1; },
  });
  const run = script => vm.runInContext(script, context);
  run(source);
  await run("loadSession()");
  return { get, run, context };
}

test("a delayed session response cannot undo logout", async () => {
  const {get, run, context} = await page();
  const wait = deferred();
  context.waiting = wait.promise;
  run("api = () => waiting");
  const request = run("loadSession()");
  run("loggedOut()");
  wait.resolve({username: "admin", secondary_required: true});
  assert.equal(await request, false);
  assert.equal(run("account"), null);
  assert.equal(get("workspace").hidden, true);
});

test("an old 401 cannot log out a newer session", async () => {
  const {run, context} = await page();
  const wait = deferred();
  context.waiting = wait.promise;
  run("fetch = () => waiting");
  const request = run("api('/api/session')");
  run("authEpoch++; account = {username: 'new-session'}");
  wait.resolve({ok:false, status:401, json:async()=>({error:"expired"})});
  await assert.rejects(request, /expired/);
  assert.equal(run("account.username"), "new-session");
});

test("slow download tools cannot replace a newer search form", async () => {
  const {get, run, context} = await page();
  const wait = deferred();
  context.waiting = wait.promise;
  run("rawAction = () => waiting");
  const opening = run("downloadDialog()");
  run("openOperation('搜索网盘', '', [{name:'keyword',label:'关键词'}], false, () => {})");
  const field = get("dialog-fields").children[0].children[0];
  field.value = "未提交的搜索词";
  wait.resolve({items:["aria2"]});
  await opening;
  assert.equal(get("dialog-title").textContent, "搜索网盘");
  assert.equal(get("dialog-fields").children[0].children[0], field);
  assert.equal(field.value, "未提交的搜索词");
});

test("slow settings cannot replace a newer form", async () => {
  const {get, run, context} = await page();
  const wait = deferred();
  context.waiting = wait.promise;
  run("api = () => waiting");
  const opening = run("settingsDialog()");
  run("openOperation('上传文件', '', [], true, () => {})");
  wait.resolve({username:"admin", secondary_required:true});
  await opening;
  assert.equal(get("dialog-title").textContent, "上传文件");
});

test("logout cancels pending download dialog", async () => {
  const {get, run, context} = await page();
  const wait = deferred();
  context.waiting = wait.promise;
  run("rawAction = () => waiting");
  const opening = run("downloadDialog()");
  run("loggedOut()");
  wait.resolve({items:["aria2"]});
  await opening;
  assert.equal(get("operation-dialog").open, false);
  assert.equal(get("workspace").hidden, true);
});
