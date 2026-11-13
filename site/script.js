(function () {
  "use strict";
  document.documentElement.classList.add("js");

  // 1. Système du visiteur : met en avant le bon téléchargement, sans rien cacher.
  var ua = navigator.userAgent || "";
  var plat = (navigator.userAgentData && navigator.userAgentData.platform) || navigator.platform || "";
  var os = null;
  if (/iPhone|iPad|iPod|Android/i.test(ua)) os = null;
  else if (/Win/i.test(plat) || /Windows/i.test(ua)) os = "windows";
  else if (/Mac/i.test(plat) || /Macintosh|Mac OS X/i.test(ua)) os = "mac";
  if (os) {
    var card = document.querySelector('[data-os="' + os + '"]');
    if (card) {
      card.classList.add("reco");
      var list = card.parentNode;
      if (list.firstElementChild !== card) list.insertBefore(card, list.firstElementChild);
    }
    var label = document.getElementById("hero-os");
    if (label) {
      label.textContent = os === "windows"
        ? "Détecté : Windows. L'édition complète est recommandée."
        : "Détecté : macOS. Version Apple Silicon (M1 et suivants), ou Lite pour un Mac Intel.";
    }
  }

  // 2. Empreintes SHA-256 (fichier généré au déploiement, même origine).
  var rows = document.querySelectorAll("[data-file]");
  function fill(map) {
    if (!map) return;
    rows.forEach(function (row) {
      var h = map[row.getAttribute("data-file")];
      if (typeof h !== "string" || !/^[0-9a-f]{64}$/i.test(h)) return;
      row.querySelector(".hash-val").textContent = h.toLowerCase();
      row.querySelector(".hash").hidden = false;
      row.querySelector(".empty").hidden = true;
      var vt = row.querySelector(".vt");
      vt.href = "https://www.virustotal.com/gui/file/" + h.toLowerCase();
      vt.hidden = false;
    });
  }
  fetch("empreintes.json", { cache: "no-cache" })
    .then(function (r) { return r.ok ? r.json() : null; })
    .then(fill)
    .catch(function () { /* message par défaut conservé */ });

  // 3. Boutons Copier.
  function select(el) {
    var r = document.createRange();
    r.selectNodeContents(el);
    var s = window.getSelection();
    s.removeAllRanges();
    s.addRange(r);
  }
  document.addEventListener("click", function (e) {
    var b = e.target.closest ? e.target.closest(".copy") : null;
    if (!b) return;
    var target = document.getElementById(b.getAttribute("data-target"));
    if (!target) return;
    var t = b.querySelector(".t");
    var old = t.textContent;
    var done = function () {
      t.textContent = "Copié";
      setTimeout(function () { t.textContent = old; }, 1800);
    };
    if (navigator.clipboard && navigator.clipboard.writeText) {
      navigator.clipboard.writeText(target.textContent).then(done, function () { select(target); });
    } else {
      select(target);
    }
  });
})();
