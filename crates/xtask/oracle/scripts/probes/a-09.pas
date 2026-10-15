{ parity probe: a-09 }
unit P_a_09;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..1, 0..1] of Integer;
begin
  Show('a[0,0,0]', a[0, 0, 0]);
end;
end.
