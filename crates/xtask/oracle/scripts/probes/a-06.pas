{ parity probe: a-06 }
unit P_a_06;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a, b: array[0..2] of Integer;
begin
  a[0] := 1;
  b := a;
  Show('b[0]', b[0]);
end;
end.
