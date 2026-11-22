using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class ModulesTests
{
    [Fact]
    public void Decodes_The_Enrolled_Bitmaps_Seen_On_The_Real_Panel()
    {
        // 88-byte reply to 7F 00 00 04 from the spare panel: header, FF FF at 4-5, and a 1 at bytes 30, 55 and 63 (keypad 1 = lowest bit of the top byte of the first group)
        var r = new byte[88]; r[0] = 0x7F; r[3] = 4; r[4] = r[5] = 0xFF; r[30] = 1; r[55] = 1; r[63] = 1;
        Assert.Equal(new[] { ("Keypad", (byte)1, 1) }, PanelModules.DecodeEnrolled(r));
        // keypad 9 and 16 (second byte of the group), input expander 3, output expander 13, serial expander 2
        r[63] = 0; r[62] = 0x81; r[67] = 0x04; r[71 - 1] = 0x10; r[79] = 0x02;
        var d = PanelModules.DecodeEnrolled(r).Select(x => $"{x.Kind} {x.Address}").ToArray();
        Assert.Equal(new[] { "Keypad 9", "Keypad 16", "Input expander 3", "Output expander 13", "Serial port expander 2" }, d);
    }

    [Fact]
    public void Decodes_Version_Replies_And_Models()
    {
        // keypad reply seen on the spare panel: hardware 1.6, boot 1.0.14, firmware 1.0.20
        var r = new byte[24]; r[0] = 0x7C; r[2] = 1; r[3] = 1; r[8] = 1; r[9] = 6; r[10] = 1; r[11] = 0; r[12] = 14; r[13] = 1; r[14] = 0; r[15] = 20;
        var m = PanelModules.DecodeVersions("Keypad", 1, r);
        Assert.Equal(("M1KP2/KP3", "1.6", "1.0.14", "1.0.20", true), (m.Model, m.Hardware, m.Boot, m.Firmware, m.Responding));
        var missing = PanelModules.DecodeVersions("Input expander", 4, null);
        Assert.False(missing.Responding); Assert.Equal("Missing or not working", missing.Firmware);
        Assert.Equal("M1XRFTWM", PanelModules.ModelFor("Input expander", 1800));
        Assert.Equal("M1XEP", PanelModules.ModelFor("Ethernet expander", 0));
    }
}
