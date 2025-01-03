const { invoke } = window.__TAURI__.core;

// 初始化元素并开始演示进度
window.addEventListener("DOMContentLoaded", async () => {
  initFooter();
  await checkForUpdates();
  registerMessageEvent();
  window.selectWorld = selectWorld;
  window.startGeneration = startGeneration;
  setupProgressListener();
  initSettings();
  initWorldPicker();
  handleBboxInput();
});

// 初始化页脚，显示当前年份和版本
async function initFooter() {
  const currentYear = new Date().getFullYear();
  document.getElementById("current-year").textContent = currentYear;

  try {
    const version = await invoke('gui_get_version');
    const footerLink = document.querySelector(".footer-link");
    footerLink.textContent = `© ${currentYear} Arnis v${version} by louis-e`;
  } catch (error) {
    console.error("获取版本失败:", error);
  }
}

// 检查更新并显示通知（如果有）
async function checkForUpdates() {
  try {
    const isUpdateAvailable = await invoke('gui_check_for_updates');
    if (isUpdateAvailable) {
      const footer = document.querySelector(".footer");
      const updateMessage = document.createElement("a");
      updateMessage.href = "https://github.com/louis-e/arnis/releases";
      updateMessage.target = "_blank";
      updateMessage.style.color = "#fecc44";
      updateMessage.style.marginTop = "-5px";
      updateMessage.style.fontSize = "0.95em";
      updateMessage.style.display = "block";
      updateMessage.style.textDecoration = "none";

      updateMessage.textContent = "有新版本可用！点击这里下载。";
      footer.style.marginTop = "15px";
      footer.appendChild(updateMessage);
    }
  } catch (error) {
    console.error("检查更新失败: ", error);
  }
}

// 注册事件监听器，用于接收来自 iframe 的边界框更新
function registerMessageEvent() {
  window.addEventListener('message', function (event) {
    const bboxText = event.data.bboxText;

    if (bboxText) {
      console.log("更新的边界框坐标:", bboxText);
      displayBboxInfoText(bboxText);
    }
  });
}

// 设置进度条监听器
function setupProgressListener() {
  const progressBar = document.getElementById("progress-bar");
  const progressMessage = document.getElementById("progress-message");
  const progressDetail = document.getElementById("progress-detail");

  window.__TAURI__.event.listen("progress-update", (event) => {
    const { progress, message } = event.payload;

    if (progress != -1) {
      progressBar.style.width = `${progress}%`;
      progressDetail.textContent = `${Math.round(progress)}%`;
    }

    if (message != "") {
      progressMessage.textContent = message;

      if (message.startsWith("错误！")) {
        progressMessage.style.color = "#fa7878";
        generationButtonEnabled = true;
      } else if (message.startsWith("完毕！")) {
        progressMessage.style.color = "#7bd864";
        generationButtonEnabled = true;
      } else {
        progressMessage.style.color = "";
      }
    }
  });
}

function initSettings() {
  // 设置
  const settingsModal = document.getElementById("settings-modal");
  const slider = document.getElementById("scale-value-slider");
  const sliderValue = document.getElementById("slider-value");
  
  // 打开设置模态框
  function openSettings() {
    settingsModal.style.display = "flex";
    settingsModal.style.justifyContent = "center";
    settingsModal.style.alignItems = "center";
  }

  // 关闭设置模态框
  function closeSettings() {
    settingsModal.style.display = "none";
  }
  
  window.openSettings = openSettings;
  window.closeSettings = closeSettings;

  // 更新滑块值显示
  slider.addEventListener("input", () => {
    sliderValue.textContent = parseFloat(slider.value).toFixed(2);
  });
}

function initWorldPicker() {
  // 世界选择器
  const worldPickerModal = document.getElementById("world-modal");
  
  // 打开世界选择器模态框
  function openWorldPicker() {
    worldPickerModal.style.display = "flex";
    worldPickerModal.style.justifyContent = "center";
    worldPickerModal.style.alignItems = "center";
  }

  // 关闭世界选择器模态框
  function closeWorldPicker() {
    worldPickerModal.style.display = "none";
  }
  
  window.openWorldPicker = openWorldPicker;
  window.closeWorldPicker = closeWorldPicker;
}

// 验证并处理边界框输入
function handleBboxInput() {
  const inputBox = document.getElementById("bbox-coords");
  const bboxInfo = document.getElementById("bbox-info");

  inputBox.addEventListener("input", function () {
      const input = inputBox.value.trim();

      if (input === "") {
          bboxInfo.textContent = "";
          bboxInfo.style.color = "";
          selectedBBox = "";
          return;
      }

      // 正则表达式验证边界框输入（支持逗号和空格分隔格式）
      const bboxPattern = /^(-?\d+(\.\d+)?)[,\s](-?\d+(\.\d+)?)[,\s](-?\d+(\.\d+)?)[,\s](-?\d+(\.\d+)?)$/;

      if (bboxPattern.test(input)) {
          const matches = input.match(bboxPattern);

          // 提取坐标（预期顺序为纬度/经度）
          const lat1 = parseFloat(matches[1]);
          const lng1 = parseFloat(matches[3]);
          const lat2 = parseFloat(matches[5]);
          const lng2 = parseFloat(matches[7]);

          // 验证纬度和经度范围（预期顺序为纬度/经度）
          if (
              lat1 >= -90 && lat1 <= 90 &&
              lng1 >= -180 && lng1 <= 180 &&
              lat2 >= -90 && lat2 <= 90 &&
              lng2 >= -180 && lng2 <= 180
          ) {
              // 输入有效，触发事件
              const bboxText = `${lat1} ${lng1} ${lat2} ${lng2}`;
              window.dispatchEvent(new MessageEvent('message', { data: { bboxText } }));

              // 更新信息文本
              bboxInfo.textContent = "自定义选择已确认！";
              bboxInfo.style.color = "#7bd864";
          } else {
              // 有效数字但顺序或范围无效
              bboxInfo.textContent = "错误：坐标超出范围或顺序不正确（需要先纬度后经度）。";
              bboxInfo.style.color = "#fecc44";
              selectedBBox = "";
          }
      } else {
          // 输入不符合要求的格式
          bboxInfo.textContent = "格式无效。请使用 'lat,lng,lat,lng' 或 'lat lng lat lng'。";
          bboxInfo.style.color = "#fecc44";
          selectedBBox = "";
      }
  });
}

// 根据纬度和经度计算边界框的“大小”（以平方米为单位）
function calculateBBoxSize(lng1, lat1, lng2, lat2) {
  // 使用 Haversine 公式或测地线公式进行近似距离计算
  const toRad = (angle) => (angle * Math.PI) / 180;
  const R = 6371000; // 地球半径（米）

  const latDistance = toRad(lat2 - lat1);
  const lngDistance = toRad(lng2 - lng1);

  const a = Math.sin(latDistance / 2) * Math.sin(latDistance / 2) +
    Math.cos(toRad(lat1)) * Math.cos(toRad(lat2)) *
    Math.sin(lngDistance / 2) * Math.sin(lngDistance / 2);
  const c = 2 * Math.atan2(Math.sqrt(a), Math.sqrt(1 - a));

  // 边框的宽度和高度
  const height = R * latDistance;
  const width = R * lngDistance;

  return Math.abs(width * height);
}

// 将经度规范化到 [-180, 180] 范围内
function normalizeLongitude(lon) {
  return ((lon + 180) % 360 + 360) % 360 - 180;
}

const threshold1 = 12332660.00;
const threshold2 = 36084700.00;
let selectedBBox = "";

// 处理传入的边界框数据
function displayBboxInfoText(bboxText) {
  let [lng1, lat1, lng2, lat2] = bboxText.split(" ").map(Number);

  // 规范化经度
  lat1 = parseFloat(normalizeLongitude(lat1).toFixed(6));
  lat2 = parseFloat(normalizeLongitude(lat2).toFixed(6));
  selectedBBox = `${lng1} ${lat1} ${lng2} ${lat2}`;

  const bboxInfo = document.getElementById("bbox-info");

  // 如果边界框为 0,0,0,0，则重置信息文本
  if (lng1 === 0 && lat1 === 0 && lng2 === 0 && lat2 === 0) {
    bboxInfo.textContent = "";
    selectedBBox = "";
    return;
  }

  // 计算所选边界框的大小
  const selectedSize = calculateBBoxSize(lng1, lat1, lng2, lat2);

  if (selectedSize > threshold2) {
    bboxInfo.textContent = "该区域非常大，可能超出典型计算限制。";
    bboxInfo.style.color = "#fa7878";
  } else if (selectedSize > threshold1) {
    bboxInfo.textContent = "该区域相当广泛，可能需要大量时间和资源。";
    bboxInfo.style.color = "#fecc44";
  } else {
    bboxInfo.textContent = "选择已确认！";
    bboxInfo.style.color = "#7bd864";
  }
}

let worldPath = "";
async function selectWorld(generate_new_world) {
  try {
    const worldName = await invoke('gui_select_world', { generateNew: generate_new_world } );
    if (worldName) {
      worldPath = worldName;
      const lastSegment = worldName.split(/[\\/]/).pop();
      document.getElementById('selected-world').textContent = lastSegment;
      document.getElementById('selected-world').style.color = "#fecc44";
    }
  } catch (error) {
    console.error(error);
    document.getElementById('selected-world').textContent = error;
    document.getElementById('selected-world').style.color = "#fa7878";
  }

  closeWorldPicker();
}

let generationButtonEnabled = true;
async function startGeneration() {
  try {
    if (generationButtonEnabled === false) {
      return;
    }

    if (!selectedBBox || selectedBBox == "0.000000 0.000000 0.000000 0.000000") {
      document.getElementById('bbox-info').textContent = "请先选择一个位置！";
      document.getElementById('bbox-info').style.color = "#fa7878";
      return;
    }

    if (
      worldPath === "No world selected" ||
      worldPath == "Invalid Minecraft world" ||
      worldPath == "The selected world is currently in use" ||
      worldPath == "Minecraft directory not found." ||
      worldPath === ""
    ) {
      document.getElementById('selected-world').textContent = "请先选择一个 Minecraft 世界！";
      document.getElementById('selected-world').style.color = "#fa7878";
      return;
    }

    var winter_mode = document.getElementById("winter-toggle").checked;
    var scale = parseFloat(document.getElementById("scale-value-slider").value);
    var floodfill_timeout = parseInt(document.getElementById("floodfill-timeout").value, 10);
    var ground_level = parseInt(document.getElementById("ground-level").value, 10);

    // 验证 floodfill_timeout 和 ground_level
    floodfill_timeout = isNaN(floodfill_timeout) || floodfill_timeout < 0 ? 20 : floodfill_timeout;
    ground_level = isNaN(ground_level) || ground_level < -62 ? 20 : ground_level;

    // 将边界框和所选世界传递给 Rust 后端
    await invoke("gui_start_generation", {
        bboxText: selectedBBox,
        selectedWorld: worldPath,
        worldScale: scale,
        groundLevel: ground_level,
        winterMode: winter_mode,
        floodfillTimeout: floodfill_timeout,
    });

    console.log("生成过程已开始。");
    generationButtonEnabled = false;
  } catch (error) {
    console.error("启动生成时出错:", error);
    generationButtonEnabled = true;
  }
}
