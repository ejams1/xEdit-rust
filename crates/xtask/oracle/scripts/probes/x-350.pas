{ parity probe: x-350 }
unit P_x_350;

function Initialize: integer;
var
  i: Integer;
  d: Double;
  s: string;
  b: Boolean;
  sl: TStringList;
  v: Variant;
begin
  sl := TStringList.Create;
  sl.Add('a');
  v := TList.Create; AddMessage(IntToStr(v.IndexOf(sl)));
  AddMessage('after');
end;
end.
