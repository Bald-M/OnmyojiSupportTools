import { adbShell } from '../../utils/adb';
import sharp from 'sharp';

export async function takeScreenshot(): Promise<Buffer> {
  const rawData = await adbShell(['screencap', '-p']);
  // 替换换行符（兼容 Windows 模拟器截图格式）
  const fixedData = Buffer.from(rawData.toString().replace(/\r\n/g, '\n'), 'binary');
  return sharp(fixedData).png().toBuffer(); // 可进一步保存或处理图像
}

