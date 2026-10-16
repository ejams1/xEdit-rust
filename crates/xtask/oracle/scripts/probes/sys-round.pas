{
  Parity fixture (issue #31): `SetRoundMode` and the rounding functions the
  mode reaches.

  `xEdit`'s `xejviScriptAdapterMisc.pas` registers `Math.GetRoundMode`,
  `SetRoundMode`, `GetPrecisionMode` and `SetPrecisionMode`; the two setters
  return the previous mode. This probe prints the mode round trip, the
  `Round`/`Trunc`/`Floor`/`Ceil` results under every mode (the halfway cases
  pin the baseline's rounding of .5, the `-2.7`/`2.1` cases separate
  round-to-nearest from toward-zero and toward-infinity), and the text of a
  division (`1/3`) so that the arithmetic the mode may reach is compared
  too. `GetPrecisionMode` is not touched here (`sys-helpers.pas`).

  The probe ends with `rmNearest` restored, so a later probe's output does
  not depend on this one's (the modes are process state, as upstream's are).
}
unit SysRoundProbe;

var
  gD: Double;

function Flag(aValue: Boolean): string;
begin
  if aValue then
    Result := 'T'
  else
    Result := 'F';
end;

procedure Log(const aTag, aText: string);
begin
  AddMessage(aTag + ' | ' + aText);
end;

procedure Rounds(const aTag: string);
begin
  Log('round', aTag + ' 2.5=' + IntToStr(Round(2.5)) + ' 3.5=' + IntToStr(Round(3.5)) + ' 2.7=' + IntToStr(Round(2.7)) + ' 2.1=' + IntToStr(Round(2.1)));
  Log('round', aTag + ' -2.5=' + IntToStr(Round(-2.5)) + ' -3.5=' + IntToStr(Round(-3.5)) + ' -2.7=' + IntToStr(Round(-2.7)) + ' -2.1=' + IntToStr(Round(-2.1)));
  Log('round', aTag + ' trunc2.7=' + IntToStr(Trunc(2.7)) + ' trunc-2.7=' + IntToStr(Trunc(-2.7)));
  Log('round', aTag + ' floor2.7=' + IntToStr(Floor(2.7)) + ' floor-2.1=' + IntToStr(Floor(-2.1)));
  Log('round', aTag + ' ceil2.2=' + IntToStr(Ceil(2.2)) + ' ceil-2.7=' + IntToStr(Ceil(-2.7)));
  gD := 1.0 / 3.0;
  Log('round', aTag + ' third=' + FloatToStrF(gD, 1, 17, 0));
  gD := 0.1 + 0.2;
  Log('round', aTag + ' 0.1+0.2=' + FloatToStrF(gD, 1, 17, 0));
  Log('round', aTag + ' roundto2.25=' + IntToStr(Round(2.25)) + ' roundto1.35=' + IntToStr(Round(1.35)));
end;

function Initialize: integer;
begin
  Result := 0;
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
  Rounds('nearest');
  Log('mode', 'set-down-prev=' + IntToStr(SetRoundMode(rmDown)));
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
  Rounds('down');
  Log('mode', 'set-up-prev=' + IntToStr(SetRoundMode(rmUp)));
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
  Rounds('up');
  Log('mode', 'set-truncate-prev=' + IntToStr(SetRoundMode(rmTruncate)));
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
  Rounds('truncate');
  Log('mode', 'set-nearest-prev=' + IntToStr(SetRoundMode(rmNearest)));
  Log('mode', 'get=' + IntToStr(GetRoundMode()));
end;

end.
