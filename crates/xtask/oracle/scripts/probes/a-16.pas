{ parity probe: a-16 }
unit P_a_16;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[1..3] of Integer;
begin
  a[1] := 2;
  a[2] := a[1] + 1;
  Show('a[2]', a[2]);
  Show('a[0]', a[0]);
end;
end.
