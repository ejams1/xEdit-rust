{ parity probe: v-slot-46 }
unit P_v_slot_46;

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
  v := VarType(sl.IndexOf('x'));
  Show('VarType(sl.IndexOf(''x''))', v);
end;
end.
