{ parity probe: a-07 }
unit P_a_07;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a, b: array of Integer;
begin
  SetLength(a, 2);
  a[0] := 1;
  b := a;
  Show('b[0]', b[0]);
end;
end.
