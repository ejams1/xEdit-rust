{ parity probe: a-20 }
unit P_a_20;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array of Integer;
  i: Integer;
begin
  SetLength(a, 3);
  for i := Low(a) to High(a) do
    a[i] := i * 2;
  Show('a[2]', a[2]);
end;
end.
