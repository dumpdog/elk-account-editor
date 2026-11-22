using Elk.Protocol;
using Xunit;

namespace Elk.Tests;

public class LiveTests
{
    private const string Mac = "00:40:9D:00:00:04", Pass = "testphrase", Code = "246801";

    [Fact]
    public void Decodes_The_Replies_Seen_On_The_Real_Panel()
    {
        // area reply captured from the spare panel: area 1 not ready, areas 2-8 ready (0x04), bytes 4-7 = 00 10 00 00
        var a = new byte[68]; a[0] = 0x7F; a[3] = 6; a[5] = 0x10; for (int i = 2; i <= 8; i++) a[8 * i] = 0x04;
        var areas = PanelLive.DecodeAreas(a);
        Assert.Equal(8, areas.Count); Assert.False(areas[0].Ready); Assert.True(areas[1].Ready); Assert.Equal("Disarmed", areas[1].ArmedName);
        // 0x69 = alarm (3) + force-arm-ready (8 << ... ) + armed away: bits 0-1 = 3, bits 2-4 = 0x08, bits 5-7 = 1
        var b = new byte[68]; b[8] = 0x20 | 0x08 | 3; var one = PanelLive.DecodeAreas(b)[0];
        Assert.True(one.Alarm); Assert.True(one.ReadyToForceArm); Assert.Equal("Armed Away", one.ArmedName);
        // zones 1-16 = 0x09 (open, violated), voltage byte 0x89 = 13.7 V
        var z = new byte[212]; var v = new byte[212]; for (int i = 1; i <= 16; i++) { z[i + 3] = 0x09; v[i + 3] = 0x89; }
        z[20] = 0x20 | 0x02 | 4;                                                  // zone 17: EOL resistor, trouble, bypassed
        var zones = PanelLive.DecodeZones(z, v);
        Assert.Equal(208, zones.Count); Assert.Equal("Violated", zones[0].State); Assert.Equal("Open (+)", zones[0].Physical); Assert.Equal(13.7, zones[0].Volts, 3);
        Assert.Equal("Trouble", zones[16].State); Assert.True(zones[16].Bypassed); Assert.Equal("EOLR (=)", zones[16].Physical); Assert.Equal("Normal", zones[100].State);
        // system: byte 4 + 18 = A.C. failure
        var s = new byte[56]; s[4 + 18] = 1; s[4 + 19] = 2;
        Assert.Equal(new[] { "A.C. Failure", "Box Tamper" }, PanelLive.DecodeSystem(s));
    }

    [Fact]
    public async Task Reads_A_Snapshot_Over_An_Open_Session_And_Stays_Open()
    {
        using var panel = new SimulatedPanel(Mac, Pass, Code);
        using var s = new ElkSession(Mac, Pass); await s.ConnectAsync("127.0.0.1", panel.Port); await s.LoginAsync(Code);
        var one = await PanelLive.ReadAsync(s); var two = await PanelLive.ReadAsync(s);          // the same session answers repeated polls
        Assert.Equal(8, one.Areas.Count); Assert.Equal("Armed Away", one.Areas[2].ArmedName); Assert.Contains("A.C. Failure", one.System);
        Assert.Equal("Violated", one.Zones[0].State); Assert.Equal(13.7, one.Zones[0].Volts, 3); Assert.Equal(one.Zones.Count, two.Zones.Count);
    }

    private async Task<(SimulatedPanel panel, ElkSession s)> Open()
    {
        var panel = new SimulatedPanel(Mac, Pass, Code);
        var s = new ElkSession(Mac, Pass); await s.ConnectAsync("127.0.0.1", panel.Port); await s.LoginAsync(Code);
        return (panel, s);
    }

    [Theory]
    [InlineData((byte)0x00)]   // not ready
    [InlineData((byte)0x08)]   // ready to force arm: the panel would bypass the open zones
    public async Task Arm_Is_Refused_Unless_The_Area_Is_Fully_Ready(byte status)
    {
        var (panel, s) = await Open(); using var p1 = panel; using var s1 = s;
        panel.AreaStatus[2] = status;
        var e = await Assert.ThrowsAsync<InvalidOperationException>(() => PanelControl.ArmCheckedAsync(s, 2, 1));
        Assert.Contains("not ready to arm", e.Message); Assert.Contains("Nothing was sent", e.Message); Assert.Contains("force arm", e.Message); Assert.Contains("Open or violated zones: 1, 2", e.Message);
        await PanelLive.ReadAsync(s);                              // let the panel finish with the session
        Assert.Empty(panel.ArmRequests);
    }

    [Fact]
    public async Task Arm_Is_Sent_When_The_Area_Is_Ready_And_Force_Skips_The_Check()
    {
        var (panel, s) = await Open(); using var p1 = panel; using var s1 = s;
        await PanelControl.ArmCheckedAsync(s, 2, 2);               // area 2 is ready in the canned status
        panel.AreaStatus[4] = 0x08;
        await PanelControl.ArmCheckedAsync(s, 4, 1, force: true);  // explicit force: no check
        await PanelLive.ReadAsync(s);
        Assert.Equal(new[] { new byte[] { 0x7F, 0, 0x40, 0, 2, 2 }, new byte[] { 0x7F, 0, 0x40, 0, 4, 1 } }, panel.ArmRequests.ToArray());
    }

    [Fact]
    public async Task Disarm_Does_Not_Need_A_Ready_Area()
    {
        var (panel, s) = await Open(); using var p1 = panel; using var s1 = s;
        panel.AreaStatus[1] = 0x00;
        await PanelControl.ArmCheckedAsync(s, 1, 0);
        await PanelLive.ReadAsync(s);
        Assert.Single(panel.ArmRequests); Assert.Equal(0, panel.ArmRequests[0][5]);
    }
}
