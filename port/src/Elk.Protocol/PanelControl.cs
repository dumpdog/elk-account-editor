namespace Elk.Protocol;

/// <summary>
/// Remote-control commands from ElkRP's "Current Status and Remote Control" window (RemoteControl.cs): toggle a zone's bypass, arm or disarm an area, silence/reset an area.
/// Each is one request; the reply is returned as received (its opcode is not fixed) or null when the panel stays silent. The caller should re-read the status to see the result.
/// </summary>
public static class PanelControl
{
    public static readonly string[] ArmLevels = { "Disarm", "Arm Away", "Arm Stay", "Arm Stay Instant", "Arm Night", "Arm Night Instant", "Arm Vacation" };

    private static async Task<byte[]?> Send(ElkSession s, byte[] payload, CancellationToken ct)
    {
        await s.SendAsync(payload, ct);
        try { return await s.ReceiveAnyAsync(ct); } catch (TimeoutException) { return null; }
    }

    /// <summary>Toggles the bypass of one zone (FF 00 00 09 00 zone area 00).</summary>
    public static Task<byte[]?> ToggleBypassAsync(ElkSession s, int zone, int area, CancellationToken ct = default)
        => zone is < 1 or > 208 || area is < 1 or > 8 ? throw new ArgumentOutOfRangeException() : Send(s, new byte[] { 0xFF, 0, 0, 9, 0, (byte)zone, (byte)area, 0 }, ct);

    /// <summary>Arms the area at a level 1-6 (away, stay, stay instant, night, night instant, vacation) or disarms it (level 0): 7F 00 40 00 area level.</summary>
    public static Task<byte[]?> ArmAsync(ElkSession s, int area, int level, CancellationToken ct = default)
        => area is < 1 or > 8 || level is < 0 or > 6 ? throw new ArgumentOutOfRangeException() : Send(s, new byte[] { 0x7F, 0, 0x40, 0, (byte)area, (byte)level }, ct);

    /// <summary>
    /// Arms only when the area is fully ready, like a keypad that warns about open zones before it will arm. The panel itself will "force arm" an area whose open zones are
    /// force-armable (status "ready to force arm") and quietly bypasses them; this refuses that case, and the not-ready case, and says which zones are open. Disarming (level 0)
    /// and an area that is already armed are passed straight through. <paramref name="force"/> skips the check (the old behaviour, used only on request).
    /// </summary>
    public static async Task<byte[]?> ArmCheckedAsync(ElkSession s, int area, int level, bool force = false, CancellationToken ct = default)
    {
        if (area is < 1 or > 8 || level is < 0 or > 6) throw new ArgumentOutOfRangeException();
        if (level > 0 && !force)
        {
            var snap = await PanelLive.ReadAsync(s, ct);
            var a = snap.Areas.FirstOrDefault(x => x.Area == area);
            if (a != null && a.Armed == 0 && !(a.Ready && !a.ReadyToForceArm))
            {
                var open = snap.Zones.Where(z => z.State.StartsWith("Violated") && !z.Bypassed).Select(z => z.Zone).Take(12).ToList();
                throw new InvalidOperationException($"Area {area} is not ready to arm ({(a.ReadyToForceArm ? "it could only be force armed, which bypasses the open zones" : "a zone is open or in trouble")}). " +
                    (open.Count > 0 ? "Open or violated zones: " + string.Join(", ", open) + ". " : "") + "Close them (or bypass them yourself) and arm again, or use force arm to arm anyway (the panel then bypasses them). Nothing was sent.");
            }
        }
        return await ArmAsync(s, area, level, ct);
    }

    /// <summary>Silences and resets the alarm in an area: FF 00 00 0A area.</summary>
    public static Task<byte[]?> SilenceAsync(ElkSession s, int area, CancellationToken ct = default)
        => area is < 1 or > 8 ? throw new ArgumentOutOfRangeException() : Send(s, new byte[] { 0xFF, 0, 0, 10, (byte)area }, ct);
}
