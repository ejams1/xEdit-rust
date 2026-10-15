{ parity probe: x-251 }
unit P_x_251;

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
  v := IntToStr(FileByIndex(0));
  AddMessage('after');
end;
end.
