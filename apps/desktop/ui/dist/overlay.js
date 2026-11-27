const T = window.__TAURI__;
    const listen = T.event.listen;
    const appWindow = T.window.getCurrentWindow();
    const { overlayTransform } = window.overlayCoords;
    const canvas = document.getElementById('c');
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    let last = null;
    let origin = { x: 0, y: 0 };
    let settled = Promise.resolve();

    function applyTransform() {
      const t = overlayTransform(dpr, origin.x, origin.y);
      ctx.setTransform(t.a, t.b, t.c, t.d, t.e, t.f);
    }

    // A pointer drawn from the wrong origin is a pointer on the wrong thing, and
    // the overlay has no surface of its own to complain on. Saying it in the
    // console of a webview nobody opens is the same as not saying it, so this
    // goes to the banner the main window polls every two seconds.
    async function reportOriginProblem(reason) {
      console.warn('overlay: no window origin available, drawing from 0,0', reason);
      try {
        await T.core.invoke('overlay_origin_unavailable', { reason: String(reason) });
      } catch (err) {
        console.error('overlay: could not report the missing origin', err);
      }
    }

    // Asked for on every resize, because the origin is not a constant: the system
    // clamps the window below the menu bar, the user drags it to another display,
    // a display gets plugged in.
    async function readOrigin() {
      try {
        const pos = await appWindow.outerPosition();
        return { x: pos.x / dpr, y: pos.y / dpr };
      } catch (err) {
        await reportOriginProblem(err);
        return { x: 0, y: 0 };
      }
    }

    async function resize() {
      origin = await readOrigin();
      canvas.width = innerWidth * dpr;
      canvas.height = innerHeight * dpr;
      applyTransform();
      // Sizing a canvas erases it, and a window resize is the one thing that
      // happens while a pointer is already on screen.
      if (last) paint(last);
    }
    settled = resize();
    addEventListener('resize', () => { settled = resize(); });

    let raf = 0;

    function paint(plan) {
      // Clearing runs without the transform: with it on, the wipe starts at the
      // window origin's negative and leaves a strip of the previous frame.
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, canvas.width, canvas.height);
      applyTransform();
      ctx.lineWidth = 3;
      ctx.strokeStyle = '#ffd166';
      ctx.font = '600 13px -apple-system, system-ui, sans-serif';
      for (const item of plan.items) {
        for (const s of item.strokes) {
          if (s.points.length < 2) continue;
          ctx.beginPath();
          ctx.moveTo(s.points[0].x, s.points[0].y);
          for (const p of s.points.slice(1)) ctx.lineTo(p.x, p.y);
          ctx.stroke();
        }
        if (item.label && item.target) {
          ctx.fillStyle = '#ffd166';
          ctx.fillText(item.label, item.target.x + 10, item.target.y - 10);
        }
      }
    }

    async function draw(plan) {
      cancelAnimationFrame(raf);
      last = plan;
      // The plan arrives in screen points and the canvas needs the window origin
      // to place them; waiting here is what keeps the first frame from being the
      // only one drawn 33 points off.
      await settled;
      let t = 0;
      const stroke = () => {
        paint(plan);
        t += 1;
        if (t < 20) raf = requestAnimationFrame(stroke);
      };
      stroke();
    }

    let caption = '';
    function setCaption(text) {
      const box = document.getElementById('cap');
      caption = text;
      box.textContent = text;
      box.className = text ? 'on' : '';
    }

    await listen('recording', (e) => {
      document.getElementById('rec').className = e.payload ? 'on' : '';
      if (e.payload) setCaption('');
    });
    // Subtitle is the caption bar for an answer spoken over the screen; Dictation
    // previews what is being transcribed. Both land here, not in the main window.
    await listen('app-event', (e) => {
      const p = e.payload || {};
      const d = p.data;
      if (p.kind === 'Subtitle' && d && d.text) setCaption(caption + d.text);
      if (p.kind === 'Dictation' && d) setCaption(d.text || '');
      if (p.kind === 'Answer') setCaption('');
    });
    await listen('pointer-plan', (e) => draw(e.payload));
    await listen('pointer-clear', () => {
      last = null;
      ctx.setTransform(1, 0, 0, 1, 0, 0);
      ctx.clearRect(0, 0, canvas.width, canvas.height);
    });
