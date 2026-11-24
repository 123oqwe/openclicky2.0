const T = window.__TAURI__;
    const listen = T.event.listen;
    const canvas = document.getElementById('c');
    const ctx = canvas.getContext('2d');
    const dpr = window.devicePixelRatio || 1;

    function resize() {
      canvas.width = innerWidth * dpr;
      canvas.height = innerHeight * dpr;
      ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    }
    resize();
    addEventListener('resize', resize);

    let raf = 0;
    function draw(plan) {
      cancelAnimationFrame(raf);
      let t = 0;
      const stroke = () => {
        ctx.clearRect(0, 0, innerWidth, innerHeight);
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
    await listen('pointer-clear', () => ctx.clearRect(0, 0, innerWidth, innerHeight));
