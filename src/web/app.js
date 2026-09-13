"use strict";
const $ = (id) => document.getElementById(id);
let account = null;
let pending = 0;
let taskPanel = null;
let taskTimer = null;
let dialogAction = null;
let dialogPending = false;
let knownDone = null;
let viewEpoch = 0;
let authEpoch = 0;
let dialogEpoch = 0;

function el(tag, text, cls) {
  const node = document.createElement(tag);
  if (text !== undefined && text !== null) node.textContent = String(text);
  if (cls) node.className = cls;
  return node;
}
function button(text, callback, cls = "chip") {
  const node = el("button", text, cls);
  node.type = "button";
  node.addEventListener("click", () =>
    Promise.resolve().then(callback).catch(showError),
  );
  return node;
}
function size(bytes) {
  if (!Number.isFinite(Number(bytes))) return "—";
  const value = Math.max(0, Number(bytes));
  if (value < 1024) return `${value} B`;
  const unit = Math.min(4, Math.floor(Math.log(value) / Math.log(1024)));
  return `${(value / 1024 ** unit).toFixed(1)} ${["B", "KB", "MB", "GB", "TB"][unit]}`;
}
function scrollChat() {
  $("chat").scrollTop = $("chat").scrollHeight;
}
function message(text, user = false) {
  $("welcome").hidden = true;
  const wrapper = el("article", null, `message ${user ? "user" : "bot"}`);
  const content = el("div", null, user ? "bubble" : "bot-content");
  if (!user) wrapper.append(el("div", "OPENLIST / 工作台", "bot-label"));
  if (text)
    content.append(user ? document.createTextNode(text) : el("p", text));
  wrapper.append(content);
  $("chat").append(wrapper);
  scrollChat();
  return content;
}
function showError(error) {
  const node = message(error.message || "操作失败，请稍后重试");
  node.classList.add("error");
}
function loggedOut() {
  authEpoch++;
  dialogEpoch++;
  account = null;
  taskPanel = null;
  knownDone = null;
  clearInterval(taskTimer);
  taskTimer = null;
  if ($("operation-dialog").open) $("operation-dialog").close();
  $("workspace").hidden = true;
  $("login-screen").hidden = false;
  clearChat();
}
async function api(path, body = {}, options = {}) {
  const epoch = authEpoch;
  const headers = { "X-Openlist-Web": "1", ...options.headers };
  if (!options.binary) headers["Content-Type"] = "application/json";
  const response = await fetch(path, {
    method: "POST",
    credentials: "same-origin",
    headers,
    body: options.binary ? body : JSON.stringify(body),
  });
  let data;
  try {
    data = await response.json();
  } catch {
    data = {
      error:
        response.status === 413
          ? "请求过大，单个上传文件不能超过 16 MB。"
          : "服务响应异常，请检查连接。",
    };
  }
  if (!response.ok) {
    if (response.status === 401 && path !== "/api/login" && epoch === authEpoch) loggedOut();
    throw new Error(data.error || "操作失败");
  }
  return data;
}
async function busy(work) {
  pending++;
  $("busy").hidden = false;
  try {
    return await work();
  } finally {
    pending--;
    $("busy").hidden = pending === 0;
  }
}
async function rawAction(action, extra = {}) {
  return api("/api/action", { action, ...extra });
}
async function perform(action, extra = {}, label) {
  const epoch = viewEpoch;
  if (label) message(label, true);
  return busy(async () => {
    const data = await rawAction(action, extra);
    if (account && epoch === viewEpoch) render(data);
    return data;
  });
}
async function loadSession() {
  const epoch = authEpoch;
  const session = await api("/api/session");
  if (epoch !== authEpoch) return false;
  account = session;
  $("login-screen").hidden = true;
  $("workspace").hidden = false;
  $("account").textContent = account.username;
  $("security-label").textContent = account.secondary_required
    ? "二级密码保护已启用"
    : "登录密码保护已启用";
  return true;
}
function safeLink(url, text) {
  try {
    const parsed = new URL(url);
    if (!["https:", "http:", "magnet:", "ed2k:"].includes(parsed.protocol))
      return null;
    const node = el("a", text, "file-link");
    node.href = parsed.href;
    node.target = "_blank";
    node.rel = "noopener noreferrer";
    return node;
  } catch {
    return null;
  }
}
function paged(container, items, renderRow, emptyText = "这里暂时没有内容。") {
  let page = 0;
  function draw() {
    container.replaceChildren();
    if (!items.length) {
      container.append(el("div", emptyText, "empty"));
      return;
    }
    items
      .slice(page * 12, (page + 1) * 12)
      .forEach((item) => container.append(renderRow(item)));
    if (items.length > 12) {
      const pager = el("div", null, "pager");
      const previous = button("← 上一页", () => {
        page--;
        draw();
      });
      previous.disabled = page === 0;
      const next = button("下一页 →", () => {
        page++;
        draw();
      });
      next.disabled = (page + 1) * 12 >= items.length;
      pager.append(
        previous,
        el(
          "span",
          `${page + 1} / ${Math.ceil(items.length / 12)} · ${items.length} 项`,
        ),
        next,
      );
      container.append(pager);
    }
  }
  draw();
}
function panel(parent, title) {
  const root = el("section", null, "result-panel");
  const heading = el("div", title, "panel-title");
  const body = el("div");
  root.append(heading, body);
  parent.append(root);
  return { root, heading, body };
}
function childPath(path, name) {
  return `${path.replace(/\/$/, "")}/${name}`;
}
function parentPath(path) {
  return path.replace(/\/+$/, "").split("/").slice(0, -1).join("/") || "/";
}

function render(data) {
  const content = message();
  switch (data.kind) {
    case "message":
      content.append(el("p", data.text));
      break;
    case "storages": {
      const view = panel(content, "你的存储空间");
      paged(
        view.body,
        data.items,
        (item) => {
          const row = el("div", null, "file-row");
          const title = item.mount_path || "/";
          const main = button(
            title,
            () => perform("browse", { path: title }, `浏览 ${title}`),
            "file-open row-main",
          );
          main.disabled = item.disabled;
          main.append(
            el(
              "small",
              item.disabled ? "已停用" : item.remark || "点击进入目录",
            ),
          );
          row.append(el("span", "▤", "row-icon"), main, el("span", "↗"));
          return row;
        },
        "没有可用的存储空间。",
      );
      content.append(
        button("浏览根目录", () => perform("browse", { path: "/" }, "浏览 /")),
      );
      break;
    }
    case "files": {
      const view = panel(content, data.path);
      const items = [...data.items].sort(
        (a, b) =>
          Number(b.is_dir) - Number(a.is_dir) ||
          a.name.localeCompare(b.name, "zh-CN"),
      );
      paged(
        view.body,
        items,
        (item) => {
          const row = el("div", null, "file-row");
          const path = childPath(data.path, item.name);
          const main = button(
            item.name,
            () =>
              perform(
                item.is_dir ? "browse" : "file",
                { path },
                `${item.is_dir ? "浏览" : "查看"} ${path}`,
              ),
            "file-open row-main",
          );
          main.append(el("small", item.is_dir ? "文件夹" : size(item.size)));
          row.append(
            el("span", item.is_dir ? "▱" : "·", "row-icon"),
            main,
            button(
              "删除",
              () =>
                openOperation(
                  "删除文件",
                  `确定删除「${path}」？文件夹会连同内容一起删除，此操作无法在工作台撤销。`,
                  [],
                  true,
                  async (_, password) => {
                    await perform(
                      "remove",
                      {
                        path: data.path,
                        name: item.name,
                        secondary_password: password,
                      },
                      `删除 ${path}`,
                    );
                    await perform("browse", { path: data.path }).catch(showError);
                  },
                ),
              "chip danger",
            ),
          );
          return row;
        },
        "这是一个空目录。",
      );
      const actions = el("div", null, "actions");
      if (data.path !== "/")
        actions.append(
          button("↑ 上一级", () =>
            perform("browse", { path: parentPath(data.path) }),
          ),
        );
      actions.append(
        button("新建文件夹", () =>
          openOperation(
            "新建文件夹",
            `位置：${data.path}`,
            [{ name: "name", label: "文件夹名称", required: true }],
            true,
            async (values, password) => {
              await perform("mkdir", {
                path: data.path,
                name: values.name,
                secondary_password: password,
              });
              await perform("browse", { path: data.path }).catch(showError);
            },
          ),
        ),
        button("上传文件", () => uploadDialog(data.path)),
        button("刷新缓存", () => refreshDialog(data.path)),
      );
      content.append(actions);
      break;
    }
    case "file": {
      const view = panel(content, data.name);
      const row = el("div", null, "empty");
      row.append(el("p", size(data.size)));
      const link = data.url && safeLink(data.url, "打开 / 下载文件 ↗");
      row.append(link || el("p", "此存储未提供可用的直链。"));
      view.body.append(row);
      break;
    }
    case "search": {
      if (data.warnings?.length)
        content.append(el("p", data.warnings.join("；"), "muted"));
      const view = panel(content, `搜索结果 · ${data.items.length} 项`);
      const filter = el("select", null, "result-filter");
      filter.setAttribute("aria-label", "筛选网盘类型");
      filter.append(new Option("全部类型", ""));
      [...new Set(data.items.map((x) => x.pan_type))]
        .sort()
        .forEach((type) => filter.append(new Option(type, type)));
      view.heading.append(filter);
      function draw() {
        paged(
          view.body,
          data.items.filter(
            (x) => !filter.value || x.pan_type === filter.value,
          ),
          (item) => {
            const row = el("div", null, "search-row");
            const main = el("div", null, "row-main");
            main.append(
              el("strong", item.name || "未命名资源"),
              el(
                "small",
                [
                  item.pan_type,
                  item.size,
                  item.password ? `提取码：${item.password}` : "",
                ]
                  .filter(Boolean)
                  .join(" · "),
              ),
            );
            const link = safeLink(item.url, "打开资源 ↗");
            if (link) main.append(link);
            main.append(el("span", item.url, "inline-url"));
            row.append(
              main,
              button("下载", () => downloadDialog(item.url)),
            );
            return row;
          },
          "没有找到匹配资源，请换个关键词或检查搜索源设置。",
        );
      }
      filter.addEventListener("change", draw);
      draw();
      break;
    }
    case "tasks": {
      const view = panel(content, "下载任务 · 每 30 秒更新");
      taskPanel = view.body;
      renderTasks(data, taskPanel);
      content.append(button("立即更新", () => pollTasks(true)));
      if (!taskTimer) taskTimer = setInterval(() => pollTasks(false), 30000);
      break;
    }
    default:
      content.append(el("p", "操作已完成。"));
  }
  scrollChat();
}
function renderTasks(data, target) {
  const currentDone = new Set(data.done.map((t) => t.id));
  if (knownDone) {
    const added = data.done.filter((t) => !knownDone.has(t.id));
    if (added.length)
      message(`有 ${added.length} 个下载任务已结束，请在任务列表查看结果。`);
  }
  knownDone = currentDone;
  const items = [
    ...data.undone.map((t) => ({ ...t, finished: false })),
    ...data.done.map((t) => ({ ...t, finished: true })),
  ];
  paged(
    target,
    items,
    (task) => {
      const row = el("div", null, "task-row");
      const main = el("div", null, "row-main");
      main.append(
        el("strong", task.name),
        el(
          "small",
          task.error || task.status || (task.finished ? "任务结束" : "进行中"),
        ),
      );
      const progress = el("progress");
      progress.max = 100;
      progress.value = Math.min(100, Math.max(0, Number(task.progress) || 0));
      progress.setAttribute("aria-label", "下载进度");
      if (!task.finished) main.append(progress);
      row.append(
        main,
        el(
          "span",
          task.finished
            ? task.error
              ? "失败"
              : "已结束"
            : `${progress.value.toFixed(0)}%`,
          "task-status",
        ),
      );
      return row;
    },
    "暂无下载任务，可以先提交一个链接。",
  );
}
let polling = false;
async function pollTasks(manual) {
  if (
    polling ||
    !account ||
    !taskPanel?.isConnected ||
    (!manual && document.hidden)
  )
    return;
  polling = true;
  try {
    const data = await rawAction("tasks");
    if (taskPanel?.isConnected) renderTasks(data, taskPanel);
  } catch (e) {
    if (manual) showError(e);
  } finally {
    polling = false;
  }
}

function openOperation(title, description, fields, sensitive, action) {
  if (dialogPending || !account) return;
  dialogEpoch++;
  $("dialog-title").textContent = title;
  $("dialog-description").textContent = description;
  $("dialog-fields").replaceChildren();
  $("dialog-error").textContent = "";
  $("secondary-password").value = "";
  $("secondary-field").hidden = !(sensitive && account.secondary_required);
  $("secondary-password").required = sensitive && account.secondary_required;
  for (const field of fields) {
    const label = el("label", field.label);
    const node = el(
      field.options
        ? "select"
        : field.type === "textarea"
          ? "textarea"
          : "input",
    );
    node.name = field.name;
    node.required = Boolean(field.required);
    if (field.options)
      field.options.forEach((v) => node.append(new Option(v, v)));
    else if (field.type !== "textarea") node.type = field.type || "text";
    if (node.type !== "file") node.value = field.value || "";
    label.append(node);
    if (field.note) label.append(el("small", field.note));
    $("dialog-fields").append(label);
  }
  dialogAction = action;
  $("dialog-submit").disabled = false;
  $("close-dialog").disabled = false;
  $("operation-dialog").showModal();
}
$("operation-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  if (dialogPending) return;
  dialogPending = true;
  $("dialog-submit").disabled = true;
  $("close-dialog").disabled = true;
  $("dialog-error").textContent = "";
  const password = $("secondary-password").value;
  $("secondary-password").value = "";
  try {
    const values = Object.fromEntries(new FormData(event.currentTarget));
    await dialogAction(values, password || undefined);
    $("operation-dialog").close();
  } catch (e) {
    $("dialog-error").textContent = e.message;
  } finally {
    dialogPending = false;
    $("dialog-submit").disabled = false;
    $("close-dialog").disabled = false;
  }
});
$("close-dialog").addEventListener("click", () => {
  if (!dialogPending) $("operation-dialog").close();
});
$("operation-dialog").addEventListener("cancel", (e) => {
  if (dialogPending) e.preventDefault();
});
$("operation-dialog").addEventListener("close", () => {
  dialogEpoch++;
  $("secondary-password").value = "";
  $("dialog-fields").replaceChildren();
  dialogAction = null;
});

async function downloadDialog(urls = "") {
  if (dialogPending || !account) return;
  const epoch = ++dialogEpoch;
  const sessionEpoch = authEpoch;
  const data = await busy(() => rawAction("tools"));
  if (epoch !== dialogEpoch || sessionEpoch !== authEpoch || !account) return;
  if (!data.items.length)
    throw new Error("OpenList 没有可用的离线下载工具，请先配置下载工具。");
  openOperation(
    "新建离线下载",
    "链接将交给 OpenList 中的下载工具处理。",
    [
      {
        name: "urls",
        label: "下载链接",
        type: "textarea",
        value: urls,
        required: true,
        note: "每行一个，最多 20 个。支持 HTTP(S)、magnet、ed2k。",
      },
      {
        name: "tool",
        label: "下载工具",
        options: data.items,
        value: data.items.includes(account.download_tool)
          ? account.download_tool
          : data.items[0],
        required: true,
      },
      {
        name: "path",
        label: "保存目录",
        value: account.download_path,
        required: true,
      },
    ],
    true,
    async (values, password) => {
      await perform(
        "download",
        {
          ...values,
          urls: values.urls
            .split(/\r?\n/)
            .map((s) => s.trim())
            .filter(Boolean),
          secondary_password: password,
        },
        "提交离线下载",
      );
      // A failed status refresh must not leave the submitted download dialog
      // open: retrying that form would submit the download a second time.
      await perform("tasks").catch(showError);
    },
  );
}
function refreshDialog(path) {
  openOperation(
    "刷新文件缓存",
    "重新读取此目录的文件列表。",
    [{ name: "path", label: "目录", value: path, required: true }],
    true,
    (values, password) =>
      perform(
        "refresh",
        { ...values, secondary_password: password },
        `刷新缓存 ${values.path}`,
      ),
  );
}
function uploadDialog(path) {
  openOperation(
    "上传文件",
    `上传到 ${path}，单个文件最大 16 MB；同名文件不会覆盖。`,
    [{ name: "file", label: "选择文件", type: "file", required: true }],
    true,
    async (values, password) => {
      const file = values.file;
      if (!(file instanceof File) || !file.name)
        throw new Error("请选择一个文件");
      if (file.size > account.upload_limit)
        throw new Error("单个文件不能超过 16 MB");
      const headers = { "Content-Type": "application/octet-stream" };
      if (password)
        headers["X-Secondary-Password"] = btoa(
          Array.from(new TextEncoder().encode(password), (b) =>
            String.fromCharCode(b),
          ).join(""),
        );
      await busy(async () => {
        const result = await api(
          `/api/upload?${new URLSearchParams({ path, name: file.name })}`,
          file,
          { binary: true, headers },
        );
        render(result);
      });
      await perform("browse", { path }).catch(showError);
    },
  );
}
async function settingsDialog() {
  if (dialogPending || !account) return;
  const epoch = ++dialogEpoch;
  if (!(await loadSession()) || epoch !== dialogEpoch || !account) return;
  openOperation(
    "工作台设置",
    "下载默认值与搜索源会保存到配置文件。账号和密码在服务器的 web 配置中管理。",
    [
      {
        name: "path",
        label: "默认下载目录",
        value: account.download_path,
        required: true,
      },
      {
        name: "tool",
        label: "默认下载工具",
        value: account.download_tool,
        required: true,
      },
      {
        name: "sources",
        label: "允许的搜索源",
        value: account.allowed_sources.join(", "),
        note: "用逗号分隔，例如 baidu, aliyun, quark, magnet；留空表示不显示任何结果。",
      },
    ],
    true,
    async (values, password) => {
      await perform("settings", {
        path: values.path,
        tool: values.tool,
        allowed_sources: values.sources
          .split(/[,，]/)
          .map((s) => s.trim())
          .filter(Boolean),
        secondary_password: password,
      });
      await loadSession().catch(showError);
    },
  );
}
async function command(text) {
  text = text.trim();
  if (!text) return;
  if (!account) return;
  const match = text.match(/^(\/\S+)(?:\s+([\s\S]*))?$/);
  const verb = match ? match[1].toLowerCase() : "/search";
  const argument = match ? (match[2] || "").trim() : text;
  switch (verb) {
    case "/browse":
      await perform(
        argument ? "browse" : "storages",
        argument ? { path: argument } : {},
        text,
      );
      break;
    case "/search":
      if (!argument)
        openOperation(
          "搜索网盘",
          "在已配置的搜索源中寻找资源。",
          [{ name: "keyword", label: "关键词", required: true }],
          false,
          (values) => perform("search", values, `搜索 ${values.keyword}`),
        );
      else await perform("search", { keyword: argument }, text);
      break;
    case "/download":
      await downloadDialog(argument);
      break;
    case "/tasks":
      await perform("tasks", {}, text);
      break;
    case "/refresh":
      refreshDialog(argument || account.download_path);
      break;
    case "/settings":
      await settingsDialog();
      break;
    case "/help":
      message(
        "/search 关键词 — 搜索网盘资源\n/browse [路径] — 浏览存储与文件\n/download [链接] — 新建离线下载\n/tasks — 查看任务，页面打开时自动更新\n/refresh [路径] — 刷新缓存\n/settings — 下载和搜索源设置\n\n普通文字会作为搜索关键词。浏览目录后，可以上传、删除文件或新建文件夹。这里仅处理 OpenList 功能，不提供通用 AI 对话。\n\n会话最长 8 小时，对话仅保留在当前页面；关闭或刷新页面会清空。文件任务仍由 OpenList 继续执行。",
      );
      break;
    default:
      message("不支持这个命令。输入 /help 查看工作台支持的操作。");
  }
}
const commandCatalog = [
  ["/search", "搜索网盘资源 · 可添加关键词"],
  ["/browse", "浏览存储与文件 · 可添加路径"],
  ["/download", "新建离线下载 · 可添加链接"],
  ["/tasks", "查看下载任务"],
  ["/refresh", "刷新文件缓存 · 可添加路径"],
  ["/settings", "下载和搜索源设置"],
  ["/help", "查看命令帮助"],
];
let suggestions = [];
let suggestionIndex = 0;
function hideSuggestions() {
  $("command-suggestions").hidden = true;
  $("message").setAttribute("aria-expanded", "false");
  $("message").removeAttribute("aria-activedescendant");
}
function selectSuggestion(index) {
  if (!suggestions[index]) return;
  $("message").value = `${suggestions[index][0]} `;
  hideSuggestions();
  $("message").focus();
}
function highlightSuggestion() {
  Array.from($("command-suggestions").children).forEach((node, index) => {
    node.setAttribute("aria-selected", String(index === suggestionIndex));
    if (index === suggestionIndex) {
      $("message").setAttribute("aria-activedescendant", node.id);
      node.scrollIntoView({block: "nearest"});
    }
  });
}
function updateSuggestions() {
  const value = $("message").value.toLowerCase();
  suggestions = /^\/\S*$/.test(value) ? commandCatalog.filter(([name]) => name.startsWith(value)) : [];
  if (!suggestions.length) { hideSuggestions(); return; }
  suggestionIndex = 0;
  const list = $("command-suggestions");
  list.replaceChildren();
  suggestions.forEach(([name, description], index) => {
    const option = el("div", null, "command-option");
    option.id = `command-option-${index}`;
    option.setAttribute("role", "option");
    option.append(el("strong", name), el("span", description));
    option.addEventListener("pointerdown", event => event.preventDefault());
    option.addEventListener("click", () => selectSuggestion(index));
    list.append(option);
  });
  list.hidden = false;
  $("message").setAttribute("aria-expanded", "true");
  highlightSuggestion();
}
$("message").addEventListener("input", updateSuggestions);
$("message").addEventListener("blur", hideSuggestions);
$("message").addEventListener("keydown", event => {
  if (event.isComposing || $("command-suggestions").hidden) return;
  if (event.key === "Escape") { event.preventDefault(); hideSuggestions(); }
  else if (event.key === "ArrowDown" || event.key === "ArrowUp") {
    event.preventDefault();
    suggestionIndex = (suggestionIndex + (event.key === "ArrowDown" ? 1 : -1) + suggestions.length) % suggestions.length;
    highlightSuggestion();
  } else if (event.key === "Enter" || event.key === "Tab") {
    event.preventDefault(); selectSuggestion(suggestionIndex);
  }
});
document
  .querySelectorAll("[data-command]")
  .forEach((node) =>
    node.addEventListener("click", () =>
      command(node.dataset.command).catch(showError),
    ),
  );
$("composer").addEventListener("submit", (event) => {
  event.preventDefault();
  hideSuggestions();
  const text = $("message").value;
  $("message").value = "";
  command(text).catch(showError);
});
$("login-form").addEventListener("submit", async (event) => {
  event.preventDefault();
  const form = event.currentTarget;
  const submit = form.querySelector("button");
  submit.disabled = true;
  authEpoch++;
  $("login-error").textContent = "";
  const values = Object.fromEntries(new FormData(form));
  form.elements.password.value = "";
  try {
    await api("/api/login", values);
    await loadSession();
    $("message").focus();
  } catch (e) {
    $("login-error").textContent = e.message;
  } finally {
    submit.disabled = false;
  }
});
$("logout").addEventListener("click", async () => {
  authEpoch++;
  dialogEpoch++;
  try {
    await api("/api/logout");
    loggedOut();
  } catch (e) {
    showError(e);
  }
});
function clearChat() {
  viewEpoch++;
  document
    .querySelectorAll("#chat > .message")
    .forEach((node) => node.remove());
  $("welcome").hidden = false;
  taskPanel = null;
  knownDone = null;
  clearInterval(taskTimer);
  taskTimer = null;
}
$("clear-chat").addEventListener("click", clearChat);
const bootstrapEpoch = authEpoch;
loadSession().catch(() => {
  if (bootstrapEpoch === authEpoch) loggedOut();
});
