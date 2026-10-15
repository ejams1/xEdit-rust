{ parity probe: a-22 }
unit P_a_22;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  a[1] := 'x';
  Show('a[1]', a[1]);
end;
end.
