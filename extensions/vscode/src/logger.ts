import type { OutputChannel } from "vscode";

export class Logger {
  constructor(private readonly output: OutputChannel) {}

  info(message: string): void {
    this.write("info", message);
  }

  warn(message: string): void {
    this.write("warn", message);
  }

  error(message: string): void {
    this.write("error", message);
  }

  show(): void {
    this.output.show(true);
  }

  dispose(): void {
    this.output.dispose();
  }

  private write(level: string, message: string): void {
    this.output.appendLine(`[${level}] ${message}`);
  }
}
