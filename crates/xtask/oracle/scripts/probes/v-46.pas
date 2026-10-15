{ parity probe: v-46 }
unit P_v_46;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
begin
  v := True div 2;
  Show('True div 2', v);
end;
end.
