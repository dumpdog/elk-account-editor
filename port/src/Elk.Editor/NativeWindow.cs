using System.Drawing;
using System.Text.Json;
using Photino.NET;

/// <summary>Shows the editor in its own desktop window (the system web view: WebView2 on Windows, WebKit on macOS) instead of a browser tab.
/// The page talks to this host through window.external.sendMessage for the things a web view does not do well: open and save file dialogs.</summary>
static class NativeWindow
{
    public static void Run(string url)
    {
        var window = new PhotinoWindow()
            .SetTitle("Elk account editor")
            .SetUseOsDefaultSize(false)
            .SetSize(new Size(1500, 950))
            .SetMinSize(900, 600)
            .Center()
            .SetResizable(true);

        window.RegisterWebMessageReceivedHandler((sender, message) =>
        {
            var w = (PhotinoWindow)sender!;
            void Reply(object o) => w.SendWebMessage(JsonSerializer.Serialize(o));
            try
            {
                using var doc = JsonDocument.Parse(message);
                var cmd = doc.RootElement.GetProperty("cmd").GetString();
                if (cmd == "openFile")
                {
                    var purpose = doc.RootElement.TryGetProperty("purpose", out var pu) ? pu.GetString() : null;
                    var picked = w.ShowOpenFile(purpose == "zones" ? "Open a zone list (Excel .xlsx or CSV)" : purpose == "plan" ? "Open a data bus plan (.json)" : "Open an ElkRP Export Account file", null, false, null);
                    if (picked == null || picked.Length == 0 || string.IsNullOrEmpty(picked[0])) { Reply(new { cmd = "cancelled" }); return; }
                    Reply(new { cmd = "opened", purpose, name = Path.GetFileName(picked[0]), b64 = Convert.ToBase64String(File.ReadAllBytes(picked[0])) });
                }
                else if (cmd == "saveFile")
                {
                    var name = doc.RootElement.GetProperty("name").GetString() ?? "export";
                    var dir = Environment.GetFolderPath(Environment.SpecialFolder.MyDocuments);
                    var purpose = doc.RootElement.TryGetProperty("purpose", out var pu) ? pu.GetString() : null;
                    var ext = doc.RootElement.TryGetProperty("ext", out var ex) ? ex.GetString() : null;
                    var path = w.ShowSaveFile((purpose == "zones" ? "Save the zone list (suggested name: " : purpose == "plan" ? "Save the data bus plan (suggested name: " : "Save edited export (suggested name: ") + name + ")", dir, null);
                    if (string.IsNullOrEmpty(path)) { Reply(new { cmd = "cancelled" }); return; }
                    if (!string.IsNullOrEmpty(ext) && string.IsNullOrEmpty(Path.GetExtension(path))) path += ext;     // the dialog may not add the extension
                    File.WriteAllBytes(path, Convert.FromBase64String(doc.RootElement.GetProperty("b64").GetString() ?? ""));
                    Reply(new { cmd = "saved", purpose, path });
                }
            }
            catch (Exception e) { Reply(new { cmd = "error", message = e.Message }); }
        });

        window.Load(url);
        window.WaitForClose();
    }
}
