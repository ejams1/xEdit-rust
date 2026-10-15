{ parity probe: a-25 }
unit P_a_25;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  a[1.5] := 7;
  Show('a[1]', a[1]);
  Show('a[2]', a[2]);
end;
end.
