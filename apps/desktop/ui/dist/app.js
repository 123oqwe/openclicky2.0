const T = window.__TAURI__;
    const invoke = T.core.invoke;
    const listen = T.event.listen;
    const $ = (id) => document.getElementById(id);

    // Every node is built with createElement/textContent. Model output, tool
    // arguments and error text never reach the DOM as markup.
    function el(tag, opts = {}, ...kids) {
      const node = document.createElement(tag);
      if (opts.class) node.className = opts.class;
      if (opts.text !== undefined) node.textContent = String(opts.text);
      for (const [k, v] of Object.entries(opts.attrs || {})) node.setAttribute(k, v);
      for (const kid of kids) if (kid) node.append(kid);
      return node;
    }

    function act(label, fn) {
      const b = el('button', { text: label });
      b.onclick = async () => {
        b.disabled = true;
        try { await fn(); } catch (e) { showError(e); }
        b.disabled = false;
        refresh();
      };
      return b;
    }

    function showError(e) {
      const box = $('answer');
      box.textContent = '';
      box.append(el('span', { class: 'err', text: String(e) }));
    }

    function notice(p) {
      const host = $('notices');
      const line = el('div', { class: 'notice' });
      line.append(el('code', { text: (p && p.code) || 'notice' }));
      line.append(document.createTextNode(' '));
      line.append(el('span', { text: (p && p.message) || '' }));
      host.prepend(line);
      while (host.childElementCount > 5) host.lastElementChild.remove();
    }

    function setState(name) {
      // `app_state` reports `as_str()` and the State event reports the serde
      // name; normalize so both paths show the same word.
      const key = String(name).toLowerCase();
      $('state').textContent = key;
      $('state').className = 'pill' + (key === 'listening' ? ' rec' : '');
    }

    function appendSubtitle(text) {
      if (!text) return;
      $('subtitle').textContent += text;
    }

    // ── 任务卡：每个 id 只建一次，之后就地打补丁 ───────────────────────
    // Rebuilding the list on every 2s poll cleared the host, so the section
    // scroll jumped back to the top and any open 详情 collapsed. Cards are now
    // created once per id and patched in place; the host is never emptied.
    const lastMsg = new Map();   // id -> 最近一条 TaskUpdated.message
    const lastCode = new Map();  // id -> error_code, when the payload carries one
    const expanded = new Set();  // 详情 打开着的任务
    const cards = new Map();     // id -> { node, parts, pendingSig, actionsSig }
    const LIVE = ['running', 'queued', 'waiting_user', 'waiting_approval'];
    const PILL = {
      waiting_approval: 'warn',
      waiting_user: 'warn',
      failed: 'bad',
      paused: 'bad',
      canceled: 'rec',
    };

    function pendingBlock(t) {
      const p = t.pending;
      const box = el('div', { class: 'pending' });
      if (p.kind === 'approval') {
        box.append(el('div', { class: 'muted', text: '要动你的文件或对外发东西，需要你点头' }));
        box.append(el('div', { text: `工具 ${p.tool || '未知'} · 风险 ${p.risk || '未知'}` }));
        if (p.summary) box.append(el('pre', { class: 'args', text: p.summary }));
        box.append(el('div', {
          class: 'muted',
          text: p.undoable ? '这一步可以撤销' : '这一步不可撤销，请确认目标没错',
        }));
        const row = el('div', { class: 'row' });
        row.append(act('同意', () => invoke('approve_task', { id: t.id })));
        row.append(act('拒绝', () => invoke('deny_task', { id: t.id })));
        box.append(row);
      } else {
        box.append(el('div', { class: 'muted', text: '它在等你回答' }));
        box.append(el('div', { text: p.question || '' }));
        const input = el('input', { attrs: { placeholder: '在这里回答' } });
        const row = el('div', { class: 'row' });
        row.append(input);
        row.append(act('回答', () => invoke('answer_question', { id: t.id, answer: input.value })));
        box.append(row);
      }
      return box;
    }

    async function loadDetail(id, host) {
      const full = await invoke('task_detail', { id });
      expanded.add(id);
      host.hidden = false;
      host.textContent = '';
      host.append(el('div', {
        class: 'muted',
        text: full.waiting_since ? `等待自 ${full.waiting_since}` : `状态 ${full.state}`,
      }));
      const p = full.pending;
      if (p) {
        host.append(el('pre', {
          class: 'args',
          text: p.kind === 'approval'
            ? `工具 ${p.tool || '?'} · 风险 ${p.risk || '?'}\n${p.summary || ''}`
            : `在等你回答：${p.question || ''}`,
        }));
      }
    }

    function cardFor(t) {
      const cached = cards.get(t.id);
      if (cached) return cached;
      const node = el('div', { class: 'card task' });
      const body = el('div', { class: 'body' });
      const name = el('b');
      const pill = el('span', { class: 'pill' });
      const title = el('div');
      title.append(name, document.createTextNode(' '), pill);
      const msg = el('div', { class: 'muted' });
      const code = el('div', { class: 'notice' });
      const pending = el('div');
      const detail = el('div', { class: 'muted' });
      detail.hidden = true;
      const actions = el('div', { class: 'actions' });
      body.append(title, msg, code, pending, detail);
      node.append(body, actions);
      const card = { node, parts: { name, pill, msg, code, pending, detail, actions }, pendingSig: null, actionsSig: null };
      cards.set(t.id, card);
      return card;
    }

    function patchCard(t) {
      const card = cardFor(t);
      const p = card.parts;
      p.name.textContent = t.name;
      p.pill.textContent = t.state;
      p.pill.className = 'pill' + (PILL[t.state] ? ' ' + PILL[t.state] : '');

      // 最近一条 TaskUpdated.message wins; `result` is the fallback so a task
      // created before this window opened still has something to show.
      const msg = lastMsg.get(t.id) || t.result || '';
      p.msg.textContent = msg;
      p.msg.hidden = !msg;

      const code = t.error_code || lastCode.get(t.id) || '';
      p.code.textContent = '';
      p.code.hidden = !code;
      if (code) {
        p.code.append(el('code', { text: code }));
        p.code.append(document.createTextNode(' 这轮没跑完'));
      }

      const pendingSig = t.pending ? JSON.stringify(t.pending) : '';
      if (pendingSig !== card.pendingSig) {
        card.pendingSig = pendingSig;
        p.pending.textContent = '';
        if (t.pending) p.pending.append(pendingBlock(t));
      }

      const actionsSig = LIVE.includes(t.state) ? 'live' : '';
      if (actionsSig !== card.actionsSig) {
        card.actionsSig = actionsSig;
        p.actions.textContent = '';
        if (actionsSig) {
          p.actions.append(act('详情', () => loadDetail(t.id, p.detail)));
          p.actions.append(act('取消', () => invoke('cancel_task', { id: t.id })));
        }
      }
      p.detail.hidden = !expanded.has(t.id);
      return card;
    }

    function renderTasks(tasks) {
      const host = $('tasks');
      const alive = new Set();
      const order = tasks.map((t) => { alive.add(t.id); return patchCard(t).node; });
      for (const [id, card] of cards) {
        if (alive.has(id)) continue;
        card.node.remove();
        cards.delete(id);
        expanded.delete(id);
        lastMsg.delete(id);
        lastCode.delete(id);
      }
      // Reorder only the nodes that are actually out of place, so untouched
      // cards keep their scroll anchor.
      let cursor = host.firstElementChild;
      for (const node of order) {
        if (node === cursor) { cursor = cursor.nextElementSibling; continue; }
        host.insertBefore(node, cursor);
      }
      $('tasksEmpty').hidden = tasks.length > 0;
      pruneApprovals(tasks);
    }

    // ── 批准卡：急事先亮出来 ────────────────────────────────────────
    // ApprovalRequest is pushed the moment a tool asks for a human, which can
    // land before the task poll sees `pending`. The card is built from the
    // event alone and back-filled with the task's 来历 (name + arguments).
    async function approvalCard(d) {
      const id = String((d && d.id) || '');
      if (!id) return;
      const host = $('approvals');
      if ($('approval-' + id)) return;
      const card = el('div', { class: 'card', attrs: { id: 'approval-' + id } });
      card.dataset.born = String(Date.now());
      const head = el('div');
      head.append(el('span', { class: 'pill warn', text: '要你点头' }));
      head.append(document.createTextNode(' '));
      head.append(el('b', { text: `工具 ${(d && d.tool) || '未知'}` }));
      card.append(head);
      card.append(el('div', { text: `风险 ${(d && d.risk) || '未知'}` }));
      const from = el('div', { class: 'muted', text: '来历：读取中…' });
      card.append(from);
      const row = el('div', { class: 'row' });
      row.append(act('同意', () => invoke('approve_task', { id })));
      row.append(act('拒绝', () => invoke('deny_task', { id })));
      card.append(row);
      host.prepend(card);
      try {
        const full = await invoke('task_detail', { id });
        const bits = [`任务「${full.name || id.slice(0, 8)}」`, `状态 ${full.state}`];
        if (full.pending && full.pending.summary) bits.push(full.pending.summary);
        from.textContent = '来历：' + bits.join(' · ');
        lastMsg.set(id, lastMsg.get(id) || full.name || '');
      } catch (e) {
        from.textContent = `来历：任务 ${id.slice(0, 8)}（${String(e)}）`;
      }
    }

    function pruneApprovals(tasks) {
      const pending = new Set(tasks.filter((t) => t.pending && t.pending.kind === 'approval').map((t) => t.id));
      for (const card of [...$('approvals').children]) {
        const id = card.id.replace('approval-', '');
        if (pending.has(id)) continue;
        // Young cards are kept: the event may beat the next poll to the row.
        if (Date.now() - Number(card.dataset.born || 0) < 3000) continue;
        card.remove();
      }
    }

    // ── 标注：overlay 画画，这里只给一张能读的清单 ──────────────────
    function renderAnnotations(plan) {
      const items = (plan && Array.isArray(plan.items)) ? plan.items : [];
      const host = $('planItems');
      host.textContent = '';
      for (const item of items) {
        const row = el('div');
        const title = item.label || item.text || `屏幕 ${item.screen}`;
        row.append(el('b', { text: title }));
        if (item.target) {
          row.append(document.createTextNode(` · (${Math.round(item.target.x)}, ${Math.round(item.target.y)})`));
        }
        if (item.text && item.text !== title) row.append(el('div', { class: 'muted', text: item.text }));
        if (item.strokes && item.strokes.length) {
          row.append(el('div', { class: 'muted', text: `${item.strokes.length} 笔标记` }));
        }
        host.append(row);
      }
      $('annotations').hidden = items.length === 0;
    }

    async function refresh() {
      try {
        const s = await invoke('app_state');
        setState(s.state);
        $('meta').textContent = `${s.model || '未选择模型'} · ${s.data_dir}`;
        $('onboard').hidden = !!s.ready && !hasMissingPerms;
        $('chat').style.opacity = s.ready ? '1' : '.5';

        const u = await invoke('usage');
        $('usage').textContent =
          `今日 ${u.daily_used.toFixed(1)}/${u.daily_limit}（${u.percent.toFixed(0)}%）` +
          ` · 约可问 ${u.about_questions} 次` + (u.blocked ? ' · 已阻断' : '');

        renderTasks(await invoke('tasks'));
      } catch (e) { showError(e); }
    }

    let hasMissingPerms = true;

    // Rebuilding these rows on a timer replaces the buttons under the user's
    // pointer, which loses the click and the focus. Redraw only when the state
    // the panel shows actually changed.
    let lastPermissionKey = null;
    let permissionProbeInFlight = false;

    function permissionKey(p) {
      return [p.screen_recording, p.accessibility, p.microphone, (p.guidance || []).join('|')]
        .join('/');
    }

    async function refreshPermissions() {
      if (permissionProbeInFlight) return null;
      permissionProbeInFlight = true;
      let p;
      try {
        p = await invoke('permissions');
      } finally {
        permissionProbeInFlight = false;
      }
      const items = [
        ['screen_recording', '屏幕录制'],
        ['accessibility', '辅助功能'],
        ['microphone', '麦克风'],
      ];
      // The microphone is not a boolean any more: "granted" is the only value
      // that means present, and "not_determined" needs a prompt, not settings.
      const present = (k, v) => (k === 'microphone' ? v === 'granted' : !!v);
      const missing = items.filter(([k]) => !present(k, p[k]));
      hasMissingPerms = missing.length > 0;
      $('permStep').hidden = missing.length === 0;

      const key = permissionKey(p);
      if (key === lastPermissionKey) return p;
      lastPermissionKey = key;

      const host = $('perms');
      host.textContent = '';
      for (const [k, label] of missing) {
        const row = el('div', { class: 'row' });
        row.append(el('span', { class: 'pill rec', text: `缺 ${label}` }));
        const askNow = k === 'microphone' && p[k] === 'not_determined';
        row.append(permissionButton(askNow ? '请求麦克风授权' : '打开系统设置', k));
        host.append(row);
      }
      const guide = $('permGuide');
      guide.textContent = '';
      for (const g of p.guidance || []) guide.append(el('div', { text: g }));
      return p;
    }

    $('recheckPerm').onclick = async () => {
      try { await refreshPermissions(); await refresh(); } catch (e) { showError(e); }
    };

    // act() re-enables its button as soon as the handler resolves. That is wrong
    // here, because a prompt is still on screen when the handler returns, so the
    // permission row owns its own enabled state.
    function permissionButton(label, kind) {
      const b = el('button', { text: label });
      b.onclick = () => askPermission(kind, b, label);
      return b;
    }

    // The outcome is what the request produced, not the state it produced it in.
    // A system prompt is answered later, so the button says a prompt is waiting
    // and the poll above takes over from there. Leaving it live would let a
    // second click open a second input stream before the first one returned.
    async function askPermission(kind, btn) {
      const label = btn ? btn.textContent : '';
      if (btn) {
        btn.disabled = true;
        btn.textContent = '已向系统请求…';
      }
      try {
        const r = await invoke('request_permission', { kind });
        const outcome = (r && r.outcome) || 'unknown';
        if (btn && outcome === 'prompted') {
          btn.textContent = '已向系统请求，请在弹框里允许';
        } else if (btn) {
          btn.disabled = false;
          btn.textContent = label;
        }
      } catch (e) {
        showError(e);
        if (btn) {
          btn.disabled = false;
          btn.textContent = label;
        }
      }
      await pollPermissions();
      await refresh();
    }

    $('fetch').onclick = async () => {
      $('fetchMsg').textContent = '正在连接…';
      try {
        const models = await invoke('list_models', { key: $('key').value });
        const sel = $('models');
        sel.textContent = '';
        for (const m of models) sel.add(new Option(m, m));
        sel.disabled = models.length === 0;
        $('save').disabled = models.length === 0;
        $('fetchMsg').textContent = `找到 ${models.length} 个模型`;
      } catch (e) {
        $('fetchMsg').textContent = String(e);
      }
    };

    async function saveProvider() {
      const model = $('models').value;
      await invoke('save_provider', { key: $('key').value, model });
      $('key').value = '';
      // The card disappearing is the only other signal, and it is easy to miss.
      $('fetchMsg').textContent = `已验证 ${model}，可以直接提问`;
      await refresh();
    }

    // A keychain that refuses the write is not a failure the user can retry.
    // Offering the encrypted file is the only way forward, and it is opt-in
    // because it puts the key on disk.
    function offerFileStore() {
      const host = $('fetchMsg');
      host.textContent = '';
      host.append(act('改为存到加密文件', async () => {
        await invoke('allow_file_store');
        await saveProvider();
      }));
    }

    $('save').onclick = async () => {
      try {
        await saveProvider();
      } catch (e) {
        const text = String(e && e.message ? e.message : e);
        showError(e);
        if (text.includes('E_KEY_NOT_STORED')) offerFileStore();
      }
    };

    $('send').onclick = async () => {
      const text = $('q').value.trim();
      if (!text) return;
      $('answer').textContent = '…';
      try { $('answer').textContent = await invoke('ask_text', { text }); }
      catch (e) { showError(e); }
      refresh();
    };
    $('q').addEventListener('keydown', (e) => { if (e.key === 'Enter') $('send').click(); });
    $('stop').onclick = async () => { try { await invoke('cancel_voice'); } catch (e) { showError(e); } };
    $('selftest').onclick = async () => {
      try { $('answer').textContent = JSON.stringify(await invoke('selftest'), null, 2); }
      catch (e) { showError(e); }
    };
    $('create').onclick = async () => {
      const goal = $('goal').value.trim();
      if (!goal) return;
      try { await invoke('create_task', { goal }); $('goal').value = ''; }
      catch (e) { showError(e); }
      refresh();
    };

    await listen('answer', (e) => {
      $('answer').textContent = e.payload;
      $('answer').className = '';
      $('subtitle').textContent = '';
      refresh();
    });
    await listen('error', (e) => { showError(e.payload); });
    await listen('recording', (e) => {
      setState(e.payload ? 'listening' : 'idle');
      if (e.payload) $('subtitle').textContent = '';
    });
    await listen('app-event', (e) => {
      const p = e.payload || {};
      const d = p.data;
      switch (p.kind) {
        case 'State': setState(String(d)); break;
        case 'Delta': appendSubtitle(d && d.text ? d.text : ''); break;
        case 'Answer':
          if (d && typeof d.text === 'string') $('answer').textContent = d.text;
          break;
        case 'Subtitle': appendSubtitle(d && d.text ? d.text : ''); break;
        case 'Dictation':
          $('subtitle').textContent = `听写：${(d && d.text) || ''}`;
          break;
        case 'Transcript':
          if (d && d.text) $('subtitle').textContent = `你说：${d.text}`;
          break;
        case 'Notice':
          notice({ code: d && d.code, message: d && d.message });
          break;
        case 'ApprovalRequest':
          notice({
            code: 'E_APPROVAL',
            message: `任务 ${String((d && d.id) || '').slice(0, 8)} 要用 ${(d && d.tool) || '工具'}（${(d && d.risk) || '?'}），等你点头`,
          });
          approvalCard(d);
          refresh();
          break;
        case 'Annotations':
          // The overlay draws the strokes from `pointer-plan`; here the main
          // window only keeps a readable list and marks the answer.
          $('answer').className = 'planned';
          notice({ code: 'plan', message: `已生成标注计划 ${(d && d.plan_id) || ''}` });
          break;
        case 'TaskUpdated':
          if (d && d.id) {
            if (d.message) lastMsg.set(String(d.id), String(d.message));
            if (d.error_code) lastCode.set(String(d.id), String(d.error_code));
          }
          refresh();
          break;
        case 'Usage':
          refresh();
          break;
        default:
          break;
      }
    });

    // `pointer-plan` / `pointer-clear` are broadcast to every window (the
    // overlay draws them), so the main window gets the same payload for free.
    await listen('pointer-plan', (e) => { renderAnnotations(e.payload); });
    await listen('pointer-clear', () => {
      renderAnnotations(null);
      $('answer').className = '';
    });

    // The permission rows only settle when the user answers a system prompt, so
    // one probe at start-up cannot see the answer. Poll while something is
    // missing and stop as soon as nothing is, because the promise on screen is
    // "come back here and it will have refreshed".
    async function pollPermissions() {
      if (hasMissingPerms) {
        try { await refreshPermissions(); } catch (e) {
          notice({ code: 'E_PERMISSIONS', message: String(e) });
        }
      }
    }

    refreshPermissions()
      .catch((e) => notice({ code: 'E_PERMISSIONS', message: String(e) }))
      .finally(() => refresh());
    setInterval(() => { pollPermissions().finally(refresh); }, 2000);
