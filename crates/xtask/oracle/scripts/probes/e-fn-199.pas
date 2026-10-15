{ parity probe: e-fn-199 }
unit P_e_fn_199;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  x: Variant;
begin
  x := Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1) + Abs(1);
  AddMessage('x = ' + VarToStr(x));
end;
end.
