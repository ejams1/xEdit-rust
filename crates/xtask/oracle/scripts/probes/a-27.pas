{ parity probe: a-27 }
unit P_a_27;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  a: array[0..2] of Integer;
begin
  Show('VarType', VarType(a));
  Show('Assigned', Assigned(a));
end;
end.
