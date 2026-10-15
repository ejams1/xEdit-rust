{ parity probe: x-302 }
unit P_x_302;

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
  sl.CaseSensitive := True; AddMessage(IntToStr(sl.IndexOf('A')));
  AddMessage('after');
end;
end.
