document.addEventListener("DOMContentLoaded", () => {
  const hostAliasEl = document.getElementById("hostAlias");
  const targetNameEl = document.getElementById("targetName");
  const fileInput = document.getElementById("fileInput");
  const selectFilesBtn = document.getElementById("selectFilesBtn");
  const dropzone = document.getElementById("dropzone");
  const uploadQueue = document.getElementById("uploadQueue");
  const queueList = document.getElementById("queueList");
  const filesList = document.getElementById("filesList");
  const refreshFilesBtn = document.getElementById("refreshFilesBtn");
  const requestModal = document.getElementById("requestModal");
  const modalSender = document.getElementById("modalSender");
  const modalFileList = document.getElementById("modalFileList");
  const acceptBtn = document.getElementById("acceptBtn");
  const declineBtn = document.getElementById("declineBtn");

  let currentPendingSessionId = null;

  // Format bytes into readable format
  function formatBytes(bytes) {
    if (bytes === 0) return "0 B";
    const k = 1024;
    const sizes = ["B", "KB", "MB", "GB"];
    const i = Math.floor(Math.log(bytes) / Math.log(k));
    return parseFloat((bytes / Math.pow(k, i)).toFixed(1)) + " " + sizes[i];
  }

  // 1. Fetch server info
  async function fetchInfo() {
    try {
      const res = await fetch("/api/info");
      if (res.ok) {
        const data = await res.json();
        hostAliasEl.textContent = data.alias || "Connected";
        targetNameEl.textContent = data.alias || "Device";
      }
    } catch (e) {
      hostAliasEl.textContent = "Offline";
    }
  }

  // 2. Fetch shared files
  async function fetchFiles() {
    try {
      const res = await fetch("/api/files");
      if (res.ok) {
        const files = await res.json();
        renderFiles(files);
      }
    } catch (e) {
      console.error("Failed to load files", e);
    }
  }

  function renderFiles(files) {
    if (!files || files.length === 0) {
      filesList.innerHTML = `<div class="empty-state">No files currently shared</div>`;
      return;
    }

    filesList.innerHTML = "";
    files.forEach(file => {
      const item = document.createElement("div");
      item.className = "file-item";
      item.innerHTML = `
        <div class="file-info">
          <span>📄</span>
          <div>
            <div class="file-name">${file.fileName}</div>
            <div class="file-size">${formatBytes(file.size)}</div>
          </div>
        </div>
        <a href="/api/files/${file.id}" download="${file.fileName}" class="btn btn-secondary btn-sm">Download</a>
      `;
      filesList.appendChild(item);
    });
  }

  // 3. File upload handling
  selectFilesBtn.addEventListener("click", () => fileInput.click());
  dropzone.addEventListener("click", (e) => {
    if (e.target !== selectFilesBtn) fileInput.click();
  });

  dropzone.addEventListener("dragover", (e) => {
    e.preventDefault();
    dropzone.classList.add("drag-over");
  });

  dropzone.addEventListener("dragleave", () => {
    dropzone.classList.remove("drag-over");
  });

  dropzone.addEventListener("drop", (e) => {
    e.preventDefault();
    dropzone.classList.remove("drag-over");
    if (e.dataTransfer.files.length > 0) {
      uploadFiles(e.dataTransfer.files);
    }
  });

  fileInput.addEventListener("change", () => {
    if (fileInput.files.length > 0) {
      uploadFiles(fileInput.files);
    }
  });

  function uploadFiles(files) {
    uploadQueue.style.display = "block";

    Array.from(files).forEach(file => {
      const queueItem = document.createElement("div");
      queueItem.className = "queue-item";
      queueItem.innerHTML = `
        <div class="queue-header">
          <span class="file-name">${file.name}</span>
          <span class="progress-status">Uploading...</span>
        </div>
        <div class="progress-bar">
          <div class="progress-fill"></div>
        </div>
      `;
      queueList.appendChild(queueItem);

      const progressFill = queueItem.querySelector(".progress-fill");
      const statusText = queueItem.querySelector(".progress-status");

      const xhr = new XMLHttpRequest();
      const formData = new FormData();
      formData.append("files", file);

      const startTime = Date.now();

      xhr.upload.addEventListener("progress", (e) => {
        if (e.lengthComputable) {
          const now = Date.now();
          const percent = Math.round((e.loaded / e.total) * 100);
          progressFill.style.width = percent + "%";

          const elapsedSec = (now - startTime) / 1000;
          const speedBytesPerSec = elapsedSec > 0.1 ? (e.loaded / elapsedSec) : 0;
          const speedFormatted = (speedBytesPerSec / (1024 * 1024)).toFixed(1) + " MB/s";

          statusText.textContent = `${percent}% • ${formatBytes(e.loaded)} / ${formatBytes(e.total)} (${speedFormatted})`;
        }
      });

      xhr.addEventListener("load", () => {
        if (xhr.status >= 200 && xhr.status < 300) {
          statusText.textContent = "✔ Done";
          statusText.style.color = "var(--success)";
          progressFill.style.background = "var(--success)";
          fetchFiles(); // refresh list
        } else {
          statusText.textContent = "✖ Failed";
          statusText.style.color = "var(--danger)";
        }
      });

      xhr.addEventListener("error", () => {
        statusText.textContent = "✖ Network Error";
        statusText.style.color = "var(--danger)";
      });

      xhr.open("POST", "/api/upload");
      xhr.send(formData);
    });
  }

  // 4. Server-Sent Events (SSE) for Real-Time Incoming Transfers
  function setupSSE() {
    const eventSource = new EventSource("/api/events");

    eventSource.addEventListener("transfer_request", (e) => {
      try {
        const data = JSON.parse(e.data);
        currentPendingSessionId = data.sessionId;
        modalSender.textContent = data.senderAlias || "Device";
        modalFileList.innerHTML = "";
        data.files.forEach(f => {
          const li = document.createElement("li");
          li.textContent = `${f.fileName} (${formatBytes(f.size)})`;
          modalFileList.appendChild(li);
        });
        requestModal.style.display = "flex";
      } catch (err) {
        console.error("Error parsing transfer_request", err);
      }
    });

    eventSource.addEventListener("files_updated", () => {
      fetchFiles();
    });

    eventSource.onerror = () => {
      setTimeout(setupSSE, 3000);
    };
  }

  acceptBtn.addEventListener("click", async () => {
    if (!currentPendingSessionId) return;
    try {
      await fetch(`/api/send/confirm/${currentPendingSessionId}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ accepted: true })
      });
    } finally {
      requestModal.style.display = "none";
      currentPendingSessionId = null;
      fetchFiles();
    }
  });

  declineBtn.addEventListener("click", async () => {
    if (!currentPendingSessionId) return;
    try {
      await fetch(`/api/send/confirm/${currentPendingSessionId}`, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ accepted: false })
      });
    } finally {
      requestModal.style.display = "none";
      currentPendingSessionId = null;
    }
  });

  refreshFilesBtn.addEventListener("click", fetchFiles);

  fetchInfo();
  fetchFiles();
  setupSSE();
});

