{ parity probe: a-39 }
unit P_a_39;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  a[0] := 1; a[1] := 2;
  Show('sum', a[0] + a[1]);
  Show('cmp', a[0] < a[1]);
end;
end.
