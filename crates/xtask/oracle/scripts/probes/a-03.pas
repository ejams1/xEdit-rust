{ parity probe: a-03 }
unit P_a_03;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array of Integer;
begin
  SetLength(a, 3);
  a[2] := 7;
  Show('a[2]', a[2]);
  Show('Length', Length(a));
  Show('High', High(a));
  Show('Low', Low(a));
  SetLength(a, 1);
  Show('High', High(a));
  SetLength(a, 5);
  Show('a[4]', a[4]);
  Show('a[0]', a[0]);
end;
end.
