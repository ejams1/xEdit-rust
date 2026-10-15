{ parity probe: v-30 }
unit P_v_30;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := True + 'x';
  Show('True + ''x''', v);
end;
end.
