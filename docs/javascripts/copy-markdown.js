// Adds a "Copy Markdown" button to the top of every page so the raw page
// source can be pasted straight into an AI agent. The source path comes from
// Material's "view source" action (content.action.view + edit_uri).
const RAW_BASE = "https://raw.githubusercontent.com/jagadeeshm007/ehai/main/";

function addCopyMarkdownButton() {
  const view = document.querySelector('a.md-content__button[href*="/raw/main/"]');
  if (!view || document.querySelector(".ehai-copy-md")) return;

  const rawUrl = RAW_BASE + view.getAttribute("href").split("/raw/main/")[1];
  const btn = document.createElement("button");
  btn.type = "button";
  btn.className = "md-content__button md-icon ehai-copy-md";
  btn.title = "Copy this page as Markdown (for AI agents)";
  btn.textContent = "Copy Markdown";

  btn.addEventListener("click", async () => {
    try {
      const res = await fetch(rawUrl);
      if (!res.ok) throw new Error(res.status);
      await navigator.clipboard.writeText(await res.text());
      btn.textContent = "Copied ✓";
    } catch (e) {
      btn.textContent = "Copy failed";
      console.error("copy-markdown:", e);
    }
    setTimeout(() => (btn.textContent = "Copy Markdown"), 2000);
  });

  view.parentNode.insertBefore(btn, view);
}

// navigation.instant swaps pages without a reload; document$ fires on each.
document$.subscribe(addCopyMarkdownButton);
