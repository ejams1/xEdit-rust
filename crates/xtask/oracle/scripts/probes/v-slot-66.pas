{ parity probe: v-slot-66 }
unit P_v_slot_66;

procedure Show(const aName: string; v: Variant);
begin
  AddMessage(aName + ' -> type ' + IntToStr(VarType(v)) + ' value [' + VarToStr(v) + ']');
end;

function Initialize: integer;
var
  v: Variant;
  sl: TStringList;
begin
  sl := TStringList.Create;
  v := VarType(sl);
  Show('VarType(sl)', v);
end;
end.
