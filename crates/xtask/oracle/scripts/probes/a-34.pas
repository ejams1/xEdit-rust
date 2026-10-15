{ parity probe: a-34 }
unit P_a_34;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2, 0..2] of Integer;
begin
  Show('High', High(a));
end;
end.
